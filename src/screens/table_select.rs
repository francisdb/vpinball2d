//! The table picker: list the `.vpx` files in the assets folder and let the player
//! choose one to play. Esc exits the game.

use crate::pinball::TablePath;
use crate::screens::Screen;
use crate::theme::widget;
use bevy::input::common_conditions::input_just_pressed;
use bevy::prelude::*;

/// Folder scanned for tables (the Bevy asset root).
const ASSETS_DIR: &str = "assets";

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::TableSelect), spawn_table_select);
    app.add_systems(
        Update,
        exit_on_escape
            .run_if(in_state(Screen::TableSelect).and(input_just_pressed(KeyCode::Escape))),
    );
}

/// The `.vpx` file names found in the assets folder, sorted.
fn available_tables() -> Vec<String> {
    let mut tables: Vec<String> = std::fs::read_dir(ASSETS_DIR)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_vpx = path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vpx"));
            is_vpx
                .then(|| path.file_name().map(|n| n.to_string_lossy().to_string()))
                .flatten()
        })
        .collect();
    tables.sort();
    tables
}

fn spawn_table_select(mut commands: Commands) {
    let tables = available_tables();
    commands
        .spawn((
            widget::ui_root("Table Select"),
            DespawnOnExit(Screen::TableSelect),
        ))
        .with_children(|parent| {
            parent.spawn(widget::header("Select a table"));
            if tables.is_empty() {
                parent.spawn(widget::label("No .vpx tables found in the assets folder"));
            }
            for table in tables {
                // Show the name without the `.vpx` extension; load the full file name.
                let label = table.strip_suffix(".vpx").unwrap_or(&table).to_string();
                // Each button picks its own table, then starts loading it.
                parent.spawn(widget::table_button(
                    label,
                    move |_: On<Pointer<Click>>,
                          mut commands: Commands,
                          mut next: ResMut<NextState<Screen>>| {
                        commands.insert_resource(TablePath::new(&table));
                        next.set(Screen::Loading);
                    },
                ));
            }
        });
}

fn exit_on_escape(mut app_exit: MessageWriter<AppExit>) {
    app_exit.write(AppExit::Success);
}
