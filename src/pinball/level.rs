//! Spawn the main level.

use crate::pinball::ball::ball;
use crate::pinball::table::{TABLE_DEPTH_VPU, TABLE_WIDTH_VPU};
use crate::pinball::triangulate::triangulate_polygon;
use crate::vpx::VpxAsset;
use crate::{
    asset_tracking::LoadResource,
    //audio::music,
    pinball::table::{TableAssets, table},
    screens::Screen,
};

use bevy::asset::RenderAssetUsages;
use bevy::color::palettes::css;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::wall::Wall;
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
            let mesh = wall_to_mesh(wall);
            //let color = css::PINK;
            let top_material = vpx_asset
                .raw
                .gamedata
                .materials
                .iter()
                .flatten()
                .find(|m| m.name == wall.top_material);
            let color = if let Some(mat) = top_material {
                bevy::color::Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
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
                Mesh2d(meshes.add(mesh)),
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

// TODO move the mesh loading to the vpx asset loader
fn wall_to_mesh(wall: &Wall) -> Mesh {
    let top_height = vpu_to_m(wall.height_top);

    // Generate vertices for top face (all with the same height)
    let num_points = wall.drag_points.len();
    let mut positions = Vec::with_capacity(num_points);
    let mut normals = Vec::with_capacity(num_points);
    let mut uvs = Vec::with_capacity(num_points);

    for point in &wall.drag_points {
        // Position (x, top_height, y) -> Bevy uses y-up
        positions.push([vpu_to_m(point.x), -vpu_to_m(point.y), top_height]);
        // Normal points up for the top face
        normals.push([0.0, 0.0, 1.0]);
        // Simple UV mapping (could be improved)
        uvs.push([point.x, point.y]);
    }

    // Triangulate the polygon using ear clipping (works for any polygon)
    // points should be counter-clockwise but this is already ensured by vpx
    let positions_2d: Vec<Vec2> = positions
        .iter()
        .map(|p| Vec2::new(p[0], p[1])) // Use x,y as 2D coordinates
        .collect();

    let indices = triangulate_polygon(&positions_2d);

    // let mesh = Mesh::from(Polyline2d::new(vertices));
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    // mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}
