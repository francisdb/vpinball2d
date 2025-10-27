//! Visual Pinball tables use legacy VBScript for scripting.
//! However, we don't want to implement a full VBScript interpreter in Rust.
//! Instead, we want to use a still supported and widely used language like Lua.
//! For now however we re-implement the script in Rust directly as a proof of concept.

use bevy::app::App;

mod exampletable;

pub(super) fn plugin(app: &mut App) {
    // depending on the TableFile resource we can choose which table script to load
    app.add_plugins((exampletable::plugin,));
}
