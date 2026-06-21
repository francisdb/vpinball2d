//! The table picker: list the tables found in the tables folder and let the
//! player choose one to play.
//!
//! By default only tables with a hand-written script are shown (the curated set
//! that renders best); a toggle switches to the full, scrollable list of every
//! table found on disk. Esc exits the game.

use crate::pinball::TablePath;
use crate::screens::Screen;
use crate::tables::{TableEntry, TableIndex, TablesDir};
use crate::theme::interaction::InteractionPalette;
use crate::theme::palette::{
    BUTTON_BACKGROUND, BUTTON_HOVERED_BACKGROUND, BUTTON_PRESSED_BACKGROUND,
    BUTTON_SELECTED_BACKGROUND, SCROLLBAR_THUMB, SCROLLBAR_TRACK,
};
use crate::theme::widget;
use bevy::input::common_conditions::input_just_pressed;
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb};
use bevy::window::CursorOptions;

/// Which input device is currently driving the picker. The keyboard and mouse take turns:
/// pressing a navigation key switches to [`InputMode::Keyboard`] (the mouse cursor hides
/// and hover highlighting is ignored, so only the keyboard selection shows); moving the
/// mouse switches back to [`InputMode::Mouse`].
#[derive(Resource, Default, Clone, Copy, PartialEq)]
enum InputMode {
    #[default]
    Mouse,
    Keyboard,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ShowAll>();
    app.init_resource::<PickerMemory>();
    app.init_resource::<InputMode>();
    app.add_systems(
        OnEnter(Screen::TableSelect),
        (spawn_table_select, enter_mouse_mode),
    );
    // Restore the cursor when leaving the picker so gameplay starts with it visible.
    app.add_systems(OnExit(Screen::TableSelect), enter_mouse_mode);
    app.add_systems(
        Update,
        (
            rebuild_content,
            detach_row_palette,
            track_input_mode,
            keyboard_nav,
            color_rows,
            scroll_to_selection,
            scroll_list,
        )
            .chain()
            .run_if(in_state(Screen::TableSelect)),
    );
    app.add_systems(
        Update,
        exit_on_escape
            .run_if(in_state(Screen::TableSelect).and_then(input_just_pressed(KeyCode::Escape))),
    );
}

/// Reset to mouse input with a visible cursor (on entering/leaving the picker).
fn enter_mouse_mode(mut mode: ResMut<InputMode>, mut cursor: Query<&mut CursorOptions>) {
    *mode = InputMode::Mouse;
    for mut cursor in &mut cursor {
        cursor.visible = true;
    }
}

/// Whether the picker shows every table (`true`) or only scripted ones (`false`).
#[derive(Resource, Default)]
struct ShowAll(bool);

/// View state that outlives leaving the picker, so returning to it (e.g. by
/// pressing Esc in a game) lands back on the same spot: the list scroll offset
/// and the table that was last opened (highlighted on return). Kept separate from
/// [`ShowAll`] so updating the scroll offset does not trigger a list rebuild.
#[derive(Resource, Default)]
struct PickerMemory {
    scroll_y: f32,
    selected: Option<String>,
}

/// The node holding the toggle, status line and table list. Its children are
/// rebuilt whenever the view mode or the (background-filled) index changes.
#[derive(Component)]
struct TableContent;

/// The scrollable container that holds the table buttons.
#[derive(Component)]
struct TableList;

/// Tags a table button (the outer node) with the table it opens, so keyboard
/// navigation can find, highlight and scroll to it.
#[derive(Component)]
struct TableRow {
    rel_path: String,
}

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
    mut memory: ResMut<PickerMemory>,
    content: Query<(Entity, Option<&Children>), With<TableContent>>,
) {
    let Ok((content, children)) = content.single() else {
        return;
    };
    // Build on first display (no children yet) and whenever inputs change.
    if children.is_some() && !show_all.is_changed() && !index.is_changed() {
        return;
    }

    // Keep a valid keyboard selection: default to the first shown table, and drop a
    // remembered selection that the current filter no longer shows.
    let selection_shown = memory.selected.as_deref().is_some_and(|sel| {
        index
            .entries
            .iter()
            .any(|e| e.rel_path == sel && (show_all.0 || e.has_script))
    });
    if !selection_shown {
        memory.selected = index
            .entries
            .iter()
            .find(|e| show_all.0 || e.has_script)
            .map(|e| e.rel_path.clone());
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
            false,
            |_: On<Pointer<Click>>,
             mut show_all: ResMut<ShowAll>,
             mut memory: ResMut<PickerMemory>| {
                show_all.0 = !show_all.0;
                // The two lists differ, so the saved offset no longer applies.
                memory.scroll_y = 0.0;
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

        // The scrollable list sits in a row next to a draggable scrollbar.
        parent
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: px(6),
                ..default()
            })
            .with_children(|row| {
                let list = row
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
                        // Restore the previous scroll offset; layout clamps it to range.
                        ScrollPosition(Vec2::new(0.0, memory.scroll_y)),
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
                            // Each button picks its own table, then starts loading it. The
                            // keyboard selection is shown with an outline (apply_row_highlight),
                            // not the resting background, so it survives mouse hover.
                            let rel_path = entry.rel_path.clone();
                            list.spawn((
                        widget::table_button(
                            label,
                            false,
                            move |_: On<Pointer<Click>>,
                                  mut commands: Commands,
                                  mut next: ResMut<NextState<Screen>>,
                                  mut memory: ResMut<PickerMemory>| {
                                memory.selected = Some(rel_path.clone());
                                commands.insert_resource(TablePath::new(&rel_path));
                                next.set(Screen::Loading);
                            },
                        ),
                        TableRow {
                            rel_path: entry.rel_path.clone(),
                        },
                    ));
                        }
                        if shown == 0 {
                            list.spawn(widget::label(format!(
                                "No .vpx tables found in {}",
                                tables_dir.0.display()
                            )));
                        }
                    })
                    .id();

                // Draggable scrollbar (bevy_ui_widgets headless widget): it writes
                // the list's ScrollPosition directly when the thumb is dragged. We
                // own the visuals - a subtle track gutter plus a palette-blue thumb.
                row.spawn((
                    Node {
                        width: px(10),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    BackgroundColor(SCROLLBAR_TRACK),
                    Scrollbar::new(list, ControlOrientation::Vertical, 32.0),
                    children![(
                        ScrollbarThumb {
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(SCROLLBAR_THUMB),
                    )],
                ));
            });
    });
}

/// Scroll the table list with the mouse wheel.
fn scroll_list(
    mut wheel: MessageReader<MouseWheel>,
    mut list: Query<&mut ScrollPosition, With<TableList>>,
    mut memory: ResMut<PickerMemory>,
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
    // Remember where we are so returning to the picker lands here again.
    memory.scroll_y = scroll.0.y;
}

/// How long a direction key must be held before it starts auto-repeating, and the
/// interval between repeats once it does.
const NAV_REPEAT_DELAY: f32 = 0.35;
const NAV_REPEAT_INTERVAL: f32 = 0.05;

/// Auto-repeat state for held up/down navigation: the active direction (-1 up, +1 down,
/// 0 none) and the countdown to the next repeat step.
#[derive(Default)]
struct NavRepeat {
    dir: i8,
    countdown: f32,
}

/// Keyboard / cabinet navigation of the table list:
/// - Down: Arrow Down or Left Shift (left flipper)
/// - Up: Arrow Up or Right Shift (right flipper)
/// - Enter: open the highlighted table
/// - Page Down / Page Up: jump to the next / previous first-letter group
///
/// Up/down auto-repeat while held: the row steps once on press, then keeps stepping on a
/// timer until the key is released.
fn keyboard_nav(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    index: Res<TableIndex>,
    show_all: Res<ShowAll>,
    mut memory: ResMut<PickerMemory>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
    mut repeat: Local<NavRepeat>,
) {
    let select = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);

    // Each navigation action as (just-pressed, held), so it can auto-repeat while held.
    // Encoded direction: +1 down, -1 up, +2 page-down, -2 page-up.
    let down = (
        keys.any_just_pressed([KeyCode::ArrowDown, KeyCode::ShiftLeft]),
        keys.any_pressed([KeyCode::ArrowDown, KeyCode::ShiftLeft]),
    );
    let up = (
        keys.any_just_pressed([KeyCode::ArrowUp, KeyCode::ShiftRight]),
        keys.any_pressed([KeyCode::ArrowUp, KeyCode::ShiftRight]),
    );
    let pgdn = (
        keys.just_pressed(KeyCode::PageDown),
        keys.pressed(KeyCode::PageDown),
    );
    let pgup = (
        keys.just_pressed(KeyCode::PageUp),
        keys.pressed(KeyCode::PageUp),
    );

    // Act immediately on a fresh press, then again on a timer while that key stays held.
    let fresh: i8 = if down.0 {
        1
    } else if up.0 {
        -1
    } else if pgdn.0 {
        2
    } else if pgup.0 {
        -2
    } else {
        0
    };
    let mut action: i8 = 0;
    if fresh != 0 {
        action = fresh;
        repeat.dir = fresh;
        repeat.countdown = NAV_REPEAT_DELAY;
    } else {
        let still_held = match repeat.dir {
            1 => down.1,
            -1 => up.1,
            2 => pgdn.1,
            -2 => pgup.1,
            _ => false,
        };
        if still_held {
            repeat.countdown -= time.delta_secs();
            if repeat.countdown <= 0.0 {
                action = repeat.dir;
                repeat.countdown = NAV_REPEAT_INTERVAL;
            }
        } else {
            repeat.dir = 0;
        }
    }

    if !(select || action != 0) {
        return;
    }

    let shown: Vec<&TableEntry> = index
        .entries
        .iter()
        .filter(|e| show_all.0 || e.has_script)
        .collect();
    if shown.is_empty() {
        return;
    }

    // Enter opens the highlighted table (or the first one if nothing is highlighted yet).
    if select {
        let target = memory
            .selected
            .clone()
            .filter(|sel| shown.iter().any(|e| &e.rel_path == sel))
            .unwrap_or_else(|| shown[0].rel_path.clone());
        commands.insert_resource(TablePath::new(&target));
        memory.selected = Some(target);
        next.set(Screen::Loading);
        return;
    }

    let current = memory
        .selected
        .as_deref()
        .and_then(|sel| shown.iter().position(|e| e.rel_path == sel));
    let idx = match current {
        // Nothing highlighted yet: the first key lands on the top entry.
        None => 0,
        Some(cur) => match action {
            1 => (cur + 1).min(shown.len() - 1),
            -1 => cur.saturating_sub(1),
            2 => next_letter_group(&shown, cur),
            _ => prev_letter_group(&shown, cur),
        },
    };
    memory.selected = Some(shown[idx].rel_path.clone());
}

/// First letter of a title, upper-cased; non-alphabetic titles group under `#`.
fn first_letter(title: &str) -> char {
    title
        .trim_start()
        .chars()
        .next()
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('#')
}

/// Index of the first entry below `cur` whose title starts with a different letter.
fn next_letter_group(shown: &[&TableEntry], cur: usize) -> usize {
    let letter = first_letter(&shown[cur].title);
    shown
        .iter()
        .enumerate()
        .skip(cur + 1)
        .find(|(_, e)| first_letter(&e.title) != letter)
        .map(|(i, _)| i)
        .unwrap_or(shown.len() - 1)
}

/// Index of the first entry of the previous first-letter group above `cur`.
fn prev_letter_group(shown: &[&TableEntry], cur: usize) -> usize {
    let letter = first_letter(&shown[cur].title);
    // Last entry of the previous group (first one going up with a different letter).
    let Some(prev_end) = (0..cur)
        .rev()
        .find(|&i| first_letter(&shown[i].title) != letter)
    else {
        return 0;
    };
    let prev_letter = first_letter(&shown[prev_end].title);
    // Walk up to the first entry of that group.
    let mut k = prev_end;
    while k > 0 && first_letter(&shown[k - 1].title) == prev_letter {
        k -= 1;
    }
    k
}

/// Take ownership of the table buttons' background colour from the shared interaction
/// palette (removed from each row's inner button as it spawns), so [`color_rows`] can
/// decide it from the selection and the active input mode without the two systems fighting.
fn detach_row_palette(added: Query<&Children, Added<TableRow>>, mut commands: Commands) {
    for children in &added {
        for &child in children {
            commands.entity(child).remove::<InteractionPalette>();
        }
    }
}

/// Switch between keyboard and mouse input. A navigation key hands control to the keyboard
/// and hides the cursor; any mouse movement hands it back and shows the cursor.
fn track_input_mode(
    keys: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    mut mode: ResMut<InputMode>,
    mut cursor: Query<&mut CursorOptions>,
) {
    let nav = keys.any_just_pressed([
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::NumpadEnter,
    ]);
    // Always drain the motion events so they don't pile up between frames.
    let moved = motion.read().count() > 0;

    let target = if nav {
        Some(InputMode::Keyboard)
    } else if moved {
        Some(InputMode::Mouse)
    } else {
        None
    };
    if let Some(target) = target
        && *mode != target
    {
        *mode = target;
        for mut cursor in &mut cursor {
            cursor.visible = target == InputMode::Mouse;
        }
    }
}

/// Colour the table buttons: the keyboard-selected row rests in the selected colour; hover
/// and press only show while the mouse is the active input. (`color_rows` fully owns the
/// background since `detach_row_palette` removed the shared interaction palette.)
fn color_rows(
    memory: Res<PickerMemory>,
    mode: Res<InputMode>,
    rows: Query<(&TableRow, &Children)>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor)>,
) {
    for (row, children) in &rows {
        let resting = if memory.selected.as_deref() == Some(row.rel_path.as_str()) {
            BUTTON_SELECTED_BACKGROUND
        } else {
            BUTTON_BACKGROUND
        };
        for &child in children {
            if let Ok((interaction, mut background)) = buttons.get_mut(child) {
                let target = match *interaction {
                    Interaction::Pressed => BUTTON_PRESSED_BACKGROUND,
                    // In keyboard mode the mouse is inactive, so hover is ignored.
                    Interaction::Hovered if *mode == InputMode::Mouse => BUTTON_HOVERED_BACKGROUND,
                    _ => resting,
                };
                if background.0 != target {
                    background.0 = target;
                }
            }
        }
    }
}

/// Scroll the list so the keyboard-selected table is visible. Uses the actual laid-out
/// node geometry (physical px) so it copes with wrapped, variable-height rows.
fn scroll_to_selection(
    memory: Res<PickerMemory>,
    mut last: Local<Option<String>>,
    rows: Query<(&TableRow, &ComputedNode, &UiGlobalTransform)>,
    mut list: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<TableList>>,
) {
    if memory.selected == *last {
        return;
    }
    *last = memory.selected.clone();
    let Some(selected) = memory.selected.as_deref() else {
        return;
    };
    let Ok((list_node, list_tf, mut scroll)) = list.single_mut() else {
        return;
    };
    let Some((_, row_node, row_tf)) = rows.iter().find(|(r, _, _)| r.rel_path == selected) else {
        return;
    };
    let list_h = list_node.size.y;
    let row_h = row_node.size.y;
    // Row top relative to the list's visible top (physical px); already includes scroll.
    let rel = (row_tf.translation.y - row_h / 2.0) - (list_tf.translation.y - list_h / 2.0);
    let delta = if rel < 0.0 {
        rel
    } else if rel + row_h > list_h {
        rel + row_h - list_h
    } else {
        0.0
    };
    if delta != 0.0 {
        // ScrollPosition is in logical px; node geometry is physical.
        scroll.0.y = (scroll.0.y + delta * list_node.inverse_scale_factor).max(0.0);
    }
}

fn exit_on_escape(mut app_exit: MessageWriter<AppExit>) {
    app_exit.write(AppExit::Success);
}
