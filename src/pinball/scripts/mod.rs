//! Visual Pinball tables use legacy VBScript for scripting.
//! However, we don't want to implement a full VBScript interpreter in Rust.
//! Instead, we want to use a still supported and widely used language like Lua.
//! For now however we re-implement the script in Rust directly as a proof of concept.
//!
//! The table is chosen at runtime, so every table's script is registered and each
//! one only activates for its own table via [`is_table`].

use crate::pinball::TablePath;
use bevy::prelude::*;

mod example_table;
mod north_pole;
mod tna;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((example_table::plugin, north_pole::plugin, tna::plugin));
}

/// File names of the tables that ship with a hand-written Rust script. The table
/// picker uses this to offer a "scripted only" view.
pub(crate) const SCRIPTED_TABLES: &[&str] = &[example_table::TABLE, north_pole::TABLE, tna::TABLE];

/// Whether the table with the given file name has a hand-written script.
pub(crate) fn has_script(file_name: &str) -> bool {
    SCRIPTED_TABLES.contains(&file_name)
}

/// Run condition: true when the currently selected table is `file_name`.
pub(super) fn is_table(file_name: &'static str) -> impl Fn(Option<Res<TablePath>>) -> bool + Clone {
    move |table_path: Option<Res<TablePath>>| {
        table_path.is_some_and(|table_path| {
            table_path.path.file_name().and_then(|n| n.to_str()) == Some(file_name)
        })
    }
}
