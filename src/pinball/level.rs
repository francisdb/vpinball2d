//! Spawn the main level.

use bevy::prelude::*;

use crate::pinball::ball::ball;
use crate::vpx::VpxAsset;
use crate::{
    asset_tracking::LoadResource,
    //audio::music,
    pinball::table::{TableAssets, table},
    screens::Screen,
};

pub(super) fn plugin(app: &mut App) {
    app.load_resource::<LevelAssets>();
}

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct LevelAssets {
    #[dependency]
    music: Handle<AudioSource>,
}

impl FromWorld for LevelAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            music: assets.load("audio/music/Fluffing A Duck.ogg"),
        }
    }
}

/// A system that spawns the main level.
pub fn spawn_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    commands.spawn((
        Name::new("Level"),
        Transform::default(),
        Visibility::default(),
        DespawnOnExit(Screen::Gameplay),
        children![
            table(&table_assets, &mut meshes, &mut materials, &assets_vpx),
            ball(&table_assets, &mut meshes, &mut materials, &assets_vpx),
            // (
            //     Name::new("Gameplay Music"),
            //     music(level_assets.music.clone())
            // )
        ],
    ));
}
