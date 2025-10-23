use bevy::prelude::*;
use loader::VpxLoader;

pub mod assets;
mod loader;
// TODO make this private again after the code has been moved
pub mod triangulate;

pub use assets::*;

pub struct VpxPlugin;

impl Plugin for VpxPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<VpxAsset>()
            .preregister_asset_loader::<VpxLoader>(&["vpx"]);
    }
    fn finish(&self, app: &mut App) {
        app.register_asset_loader(VpxLoader {});
    }
}
