//! Visual Pinball tables use legacy VBScript for scripting.
//! However, we don't want to implement a full VBScript interpreter in Rust.
//! Instead, we want to use a still supported and widely used language like Lua.
//! For now however we re-implement the script in Rust directly as a proof of concept.

use crate::pinball::TablePath;
use crate::pinball::table::TableAssets;
use crate::vpx::VpxAsset;
use bevy::prelude::*;

mod example_table;
mod north_pole;
mod tna;

pub(super) fn plugin(app: &mut App) {
    let table_path = app.world().resource::<TablePath>();
    match table_path.path.file_name().unwrap().to_str().unwrap() {
        "exampleTable.vpx" => {
            app.add_plugins((example_table::plugin,));
        }
        "North Pole (Playmatic 1967) v600.vpx" => {
            app.add_plugins((north_pole::plugin,));
        }
        "Total Nuclear Annihilation (Spooky 2017) VPW v2.3.vpx" => {
            app.add_plugins((tna::plugin,));
        }
        other => {
            warn!("No script available for table file: {}", other);
        }
    }
}

pub(super) fn load_sound(
    table_assets: &Res<TableAssets>,
    assets_vpx: &Res<Assets<VpxAsset>>,
    name: &str,
) -> Handle<AudioSource> {
    assets_vpx
        .get(&table_assets.vpx)
        .unwrap()
        .named_sounds
        .get(name)
        .unwrap_or_else(|| panic!("Sound {name} not found"))
        .clone()
}
