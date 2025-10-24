//! Spawn the main level.

use crate::pinball::ball::{BALL_RADIUS_M, ball};
use crate::pinball::table::{TABLE_DEPTH_VPU, TABLE_WIDTH_VPU};
use crate::vpx::VpxAsset;
use crate::{
    asset_tracking::LoadResource,
    //audio::music,
    pinball::table::{TableAssets, table},
    screens::Screen,
};
use avian2d::math::Vector;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::math::Affine2;
use bevy::mesh::Indices;
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
                0,
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
            ));
            // parent.spawn(ball(
            //     4,
            //     &table_assets,
            //     &mut meshes,
            //     &mut materials,
            //     &assets_vpx,
            // ));
        })
        .with_children(|parent| {
            vpx_asset
                .raw
                .gameitems
                .iter()
                .filter_map(|item| match item {
                    vpin::vpx::gameitem::GameItemEnum::Wall(wall) => Some(wall),
                    _ => None,
                })
                // for now we only load walls on the floor level
                //.filter(|w| w.height_bottom == 0.0)
                .for_each(|wall| {
                    let name = Name::from(format!("Wall {}", wall.name));
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
                    let texture = vpx_asset.named_images.get(wall.image.as_str()).cloned();
                    println!(
                        "Wall {}: texture {} collidable {}",
                        wall.name, wall.image, wall.is_collidable
                    );
                    let mut mat = ColorMaterial {
                        color: color.into(),
                        alpha_mode: AlphaMode2d::Opaque,
                        texture,
                        // TODO adjust UV scale properly, how doe vpinball do this?
                        uv_transform: Affine2::from_scale(Vec2::splat(0.01)),
                    };
                    if !wall.is_top_bottom_visible && !wall.is_side_visible {
                        mat.alpha_mode = AlphaMode2d::Blend;
                        mat.color = color.with_alpha(0.5).into();
                    }
                    let material = materials.add(mat);
                    let transform = Transform::from_xyz(
                        -table_width_m / 2.0,
                        table_depth_m / 2.0,
                        wall.height_top,
                    );
                    if wall.is_collidable && wall.height_bottom < BALL_RADIUS_M * 2.0 {
                        let mesh_mesh = meshes.get(mesh).unwrap();
                        let vertices: Vec<Vector> = mesh_mesh
                            .attribute(Mesh::ATTRIBUTE_POSITION)
                            .unwrap()
                            .as_float3()
                            .unwrap()
                            .iter()
                            .map(|v| Vector::new(v[0], v[1]))
                            .collect();
                        let indices: Vec<[u32; 2]> = match mesh_mesh.indices().unwrap() {
                            Indices::U16(idx) => idx
                                .chunks_exact(3)
                                .map(|i| [i[0] as u32, i[1] as u32])
                                .collect(),
                            Indices::U32(idx) => {
                                idx.chunks_exact(3).map(|i| [i[0], i[1]]).collect()
                            }
                        };
                        println!(
                            "Wall {}: creating collider with {} vertices and {} indices",
                            wall.name,
                            vertices.len(),
                            indices.len()
                        );
                        // we have to duplicate the first vertex at the end to close the loop
                        let mut vertices = vertices;
                        vertices.push(vertices[0]);
                        let collider = Collider::polyline(vertices, None);
                        parent.spawn((
                            name,
                            Mesh2d(mesh.clone()),
                            MeshMaterial2d(material),
                            transform,
                            RigidBody::Static,
                            Restitution::from(wall.elasticity),
                            Friction::from(wall.friction),
                            collider,
                        ));
                    } else {
                        parent.spawn((
                            name,
                            Mesh2d(mesh.clone()),
                            MeshMaterial2d(material),
                            transform,
                        ));
                    }
                });
        });
}
