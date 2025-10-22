//! Spawn the main level.

use crate::pinball::ball::ball;
use crate::pinball::table::{TABLE_DEPTH_M, TABLE_WIDTH_M};
use crate::vpx::VpxAsset;
use crate::{
    asset_tracking::LoadResource,
    //audio::music,
    pinball::table::{TableAssets, table},
    screens::Screen,
};
use avian2d::prelude::{Collider, RigidBody};
use bevy::asset::RenderAssetUsages;
use bevy::color::palettes::css;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;

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
    let default_wall_material = materials.add(ColorMaterial {
        color: css::BLACK.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });
    let wall_bundles = vpx_asset
        .raw
        .gameitems
        .iter()
        .filter_map(|item| match item {
            vpin::vpx::gameitem::GameItemEnum::Wall(wall) => Some(wall),
            _ => None,
        })
        .map(|wall| {
            let mut polygon = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            );
            let mut v_pos = vec![[0.0, 0.0, 0.0]];
            wall.drag_points.iter().for_each(|v| {
                v_pos.push([
                    v.x * 0.001 - TABLE_WIDTH_M / 2.0,
                    -v.y * 0.001 + TABLE_DEPTH_M / 2.0,
                    0.0,
                ]);
            });
            let indices: Vec<u32> = (1..=wall.drag_points.len() as u32)
                .collect::<Vec<u32>>()
                .windows(3)
                .flat_map(|w| vec![w[0], w[1], w[2]])
                .collect();
            polygon.insert_attribute(Mesh::ATTRIBUTE_POSITION, v_pos);
            polygon.insert_indices(Indices::U32(indices));

            // all vpinball coordinates are in mm, convert to meters
            // vpinball uses a coordinate system with +Y down, so invert Y here
            let vertices = wall
                .drag_points
                .iter()
                .map(|v| {
                    Vec2::new(
                        v.x * 0.001 - TABLE_WIDTH_M / 2.0,
                        -v.y * 0.001 + TABLE_DEPTH_M / 2.0,
                    )
                })
                .collect::<Vec<_>>();
            // let mesh = Mesh::from(Polyline2d::new(vertices));
            let mesh = polygon;
            (
                Name::from(format!("Wall {}", wall.name)),
                Mesh2d(meshes.add(mesh)),
                MeshMaterial2d(default_wall_material.clone()),
                // TODO the origin for vpinball is the top-left corner
                Transform::from_xyz(0.0, 0.0, wall.height_top),
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
