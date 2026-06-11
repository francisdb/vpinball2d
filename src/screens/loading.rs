//! Loads the selected table's assets, then enters gameplay.

use crate::pinball::TablePath;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::theme::widget;
use crate::vpx::VpxLoadProgress;
use bevy::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(Screen::Loading),
        (start_loading_table, spawn_loading_screen),
    );
    app.add_systems(
        Update,
        (update_progress_bar, enter_gameplay).run_if(in_state(Screen::Loading)),
    );
}

/// Kick off loading the selected table's vpx (and its image/sound dependencies).
fn start_loading_table(
    mut commands: Commands,
    table_path: Res<TablePath>,
    assets: Res<AssetServer>,
    progress: Res<VpxLoadProgress>,
) {
    // Forget the previous table's counters: the bar would otherwise read full
    // until the new load starts counting.
    progress.reset();
    let file_name = table_path.path.to_string_lossy().to_string();
    commands.insert_resource(TableAssets {
        // Load through the `tables` asset source rooted at the tables folder.
        vpx: assets.load(format!("{}://{}", crate::tables::TABLES_SOURCE, file_name)),
        file_name,
    });
}

/// The filled part of the loading bar; its width tracks the load progress.
#[derive(Component)]
struct LoadingBarFill;

fn spawn_loading_screen(mut commands: Commands) {
    commands.spawn((
        widget::ui_root("Loading"),
        DespawnOnExit(Screen::Loading),
        children![
            widget::label("Loading..."),
            (
                Name::new("Loading bar"),
                Node {
                    width: px(400),
                    height: px(14),
                    padding: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(7)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.16, 0.16, 0.18)),
                children![(
                    LoadingBarFill,
                    Node {
                        width: percent(0),
                        height: percent(100),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(crate::theme::palette::LABEL_TEXT),
                )],
            ),
        ],
    ));
}

/// Tracks the loader's shared counters (a vpx is one opaque asset to bevy, so the
/// loader counts images/sounds/meshes itself, see [`VpxLoadProgress`]).
fn update_progress_bar(
    progress: Res<VpxLoadProgress>,
    mut fill: Query<&mut Node, With<LoadingBarFill>>,
) {
    let Some(fraction) = progress.fraction() else {
        return;
    };
    for mut node in &mut fill {
        node.width = percent(fraction * 100.0);
    }
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
