//! Visual Pinball tables use legacy VBScript for scripting.
//! However, we don't want to implement a full VBScript interpreter in Rust.
//! Instead, we want to use a still supported and widely used language like Lua.
//! For now however we re-implement the script in Rust directly as a proof of concept.

use crate::pinball::table::TableAssets;
use crate::vpx::VpxAsset;
use bevy::prelude::*;

mod example_table;
mod north_pole;

pub(super) fn plugin(app: &mut App) {
    // depending on the TableFile resource we can choose which table script to load
    // if table_assets.file_name == "ExampleTable.vpx" {
    //app.add_plugins((example_table::plugin,));
    // }
    app.add_plugins((north_pole::plugin,));
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
        .expect(format!("Sound {} not found", name).as_str())
        .clone()
}
