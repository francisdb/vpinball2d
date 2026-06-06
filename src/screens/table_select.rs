//! The table picker: list the tables found in the tables folder and let the
//! player choose one to play.
//!
//! By default only tables with a hand-written script are shown (the curated set
//! that renders best); a toggle switches to the full, scrollable list of every
//! table found on disk. Esc exits the game.

use crate::pinball::TablePath;
use crate::screens::Screen;
use crate::tables::{TableIndex, TablesDir};
use crate::theme::widget;
use bevy::input::common_conditions::input_just_pressed;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ShowAll>();
    app.add_systems(OnEnter(Screen::TableSelect), spawn_table_select);
    app.add_systems(OnExit(Screen::TableSelect), reset_show_all);
    app.add_systems(
        Update,
        (rebuild_content, scroll_list).run_if(in_state(Screen::TableSelect)),
    );
    app.add_systems(
        Update,
        exit_on_escape
            .run_if(in_state(Screen::TableSelect).and(input_just_pressed(KeyCode::Escape))),
    );
}

/// Whether the picker shows every table (`true`) or only scripted ones (`false`).
#[derive(Resource, Default)]
struct ShowAll(bool);

/// The node holding the toggle, status line and table list. Its children are
/// rebuilt whenever the view mode or the (background-filled) index changes.
#[derive(Component)]
struct TableContent;

/// The scrollable container that holds the table buttons.
#[derive(Component)]
struct TableList;

fn spawn_table_select(mut commands: Commands) {
    commands
        .spawn((
            widget::ui_root("Table Select"),
            DespawnOnExit(Screen::TableSelect),
        ))
        .with_children(|parent| {
            parent.spawn(widget::header("Select a table"));
            parent.spawn((
                TableContent,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(12),
                    ..default()
                },
            ));
        });
}

/// Rebuild the toggle, status line and table list from the current view mode and
/// index. Runs on first display, when the index finishes loading, and on toggle.
fn rebuild_content(
    mut commands: Commands,
    show_all: Res<ShowAll>,
    index: Res<TableIndex>,
    tables_dir: Res<TablesDir>,
    content: Query<(Entity, Option<&Children>), With<TableContent>>,
) {
    let Ok((content, children)) = content.single() else {
        return;
    };
    // Build on first display (no children yet) and whenever inputs change.
    if children.is_some() && !show_all.is_changed() && !index.is_changed() {
        return;
    }
    if let Some(children) = children {
        for &child in children {
            commands.entity(child).despawn();
        }
    }

    let total = index.entries.len();
    let scripted = index.entries.iter().filter(|e| e.has_script).count();
    let show_all = show_all.0;

    commands.entity(content).with_children(|parent| {
        // Toggle between the scripted-only and full lists.
        let toggle = if show_all {
            "Show scripted tables only"
        } else {
            "Show all tables"
        };
        parent.spawn(widget::table_button(
            toggle,
            |_: On<Pointer<Click>>, mut show_all: ResMut<ShowAll>| {
                show_all.0 = !show_all.0;
            },
        ));

        let status = if show_all {
            if index.indexed {
                format!("All {total} tables ( * = has a script )")
            } else {
                format!("All {total} tables (reading names...)")
            }
        } else {
            format!("{scripted} tables with a script")
        };
        parent.spawn(widget::label(status));

        parent
            .spawn((
                TableList,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(8),
                    width: px(680),
                    max_height: px(560),
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
            ))
            .with_children(|list| {
                let mut shown = 0;
                for entry in index.entries.iter().filter(|e| show_all || e.has_script) {
                    shown += 1;
                    let label = if show_all && entry.has_script {
                        format!("* {}", entry.title)
                    } else {
                        entry.title.clone()
                    };
                    // Each button picks its own table, then starts loading it.
                    let rel_path = entry.rel_path.clone();
                    list.spawn(widget::table_button(
                        label,
                        move |_: On<Pointer<Click>>,
                              mut commands: Commands,
                              mut next: ResMut<NextState<Screen>>| {
                            commands.insert_resource(TablePath::new(&rel_path));
                            next.set(Screen::Loading);
                        },
                    ));
                }
                if shown == 0 {
                    list.spawn(widget::label(format!(
                        "No .vpx tables found in {}",
                        tables_dir.0.display()
                    )));
                }
            });
    });
}

/// Scroll the table list with the mouse wheel.
fn scroll_list(
    mut wheel: MessageReader<MouseWheel>,
    mut list: Query<&mut ScrollPosition, With<TableList>>,
) {
    let Ok(mut scroll) = list.single_mut() else {
        return;
    };
    for event in wheel.read() {
        let delta = match event.unit {
            MouseScrollUnit::Line => event.y * 24.0,
            MouseScrollUnit::Pixel => event.y,
        };
        scroll.0.y = (scroll.0.y - delta).max(0.0);
    }
}

/// Default back to the scripted-only view when leaving the picker.
fn reset_show_all(mut show_all: ResMut<ShowAll>) {
    show_all.0 = false;
}

fn exit_on_escape(mut app_exit: MessageWriter<AppExit>) {
    app_exit.write(AppExit::Success);
}
