//! Spawn the main level.

use crate::pinball::ball::ball;
use crate::pinball::table::{TABLE_DEPTH_VPU, TABLE_WIDTH_VPU};
use crate::vpx::VpxAsset;
use crate::{
    asset_tracking::LoadResource,
    //audio::music,
    pinball::table::{TableAssets, table},
    screens::Screen,
};

use bevy::color::palettes::css;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::vpu_to_m;

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
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let table_width_m = vpu_to_m(TABLE_WIDTH_VPU);
    let table_depth_m = vpu_to_m(TABLE_DEPTH_VPU);
    let wall_bundles = vpx_asset
        .raw
        .gameitems
        .iter()
        .filter_map(|item| match item {
            vpin::vpx::gameitem::GameItemEnum::Wall(wall) => Some(wall),
            _ => None,
        })
        //.filter(|w| w.name.contains("Wall350"))
        .map(|wall| {
            let mesh = vpx_asset
                .named_meshes
                .get(VpxAsset::wall_mesh_sub_path(&wall.name).as_str())
                .unwrap();
            //let color = css::PINK;
            let top_material = vpx_asset
                .raw
                .gamedata
                .materials
                .iter()
                .flatten()
                .find(|m| m.name == wall.top_material);
            let color = if let Some(mat) = top_material {
                Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
            } else {
                css::PINK
            };
            let material = materials.add(ColorMaterial {
                color: color.into(),
                alpha_mode: AlphaMode2d::Opaque,
                texture: None,
                ..default()
            });
            (
                Name::from(format!("Wall {}", wall.name)),
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                // The origin for vpinball is the top-left corner with y pointing downwards
                // We have y pointing upwards and origin at center of table
                Transform::from_xyz(-table_width_m / 2.0, table_depth_m / 2.0, wall.height_top),
                // RigidBody::Static,
                // Collider::rectangle(wall_width_m, wall_height_m),
            )
        })
        .collect::<Vec<_>>();

    // TODO the walls should probably be children of the table

    commands
        .spawn((
            Name::new("Level"),
            Transform::default(),
            Visibility::default(),
            DespawnOnExit(Screen::Gameplay),
            children![table(
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
            )],
        ))
        .with_children(|parent| {
            parent.spawn(ball(
                3,
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
            ));
            parent.spawn(ball(
                4,
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
            ));
        })
        .with_children(|parent| {
            for wall_bundle in wall_bundles {
                parent.spawn(wall_bundle);
            }
        });
}
