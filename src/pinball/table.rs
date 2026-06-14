//! Table-specific behavior.

use crate::pinball::lightmap::PlayfieldLightMaterial;
use crate::pinball::playfield::playfield;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::units::vpu_to_m;

// Typical pinball wall thickness is 3/4 inch = 19.05mm
const WALL_THICKNESS_M: f32 = 0.01905;

pub(super) fn plugin(_app: &mut App) {
    // `TableAssets` is inserted by the loading screen once a table is chosen; see
    // `screens::loading`.
}

/// The pinball table
pub(crate) fn table(
    // max_speed: f32,
    table_assets: &TableAssets,
    // texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    playfield_materials: &mut ResMut<Assets<PlayfieldLightMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    light_map: Handle<Image>,
    assets_vpx: &Res<Assets<VpxAsset>>,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let default_wall_material = materials.add(ColorMaterial {
        color: css::BLACK.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });

    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);

    // The desktop backdrop (the full-window image with the score windows and
    // playfield cutout) is spawned by `level::spawn_desktop_backdrop`.
    (
        Table,
        Name::from("Table"),
        Transform::default(),
        Visibility::default(),
        children![
            playfield(vpx_asset, meshes, playfield_materials, images, light_map),
            (
                Name::from("Bottom Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    table_width_m + 2.0 * WALL_THICKNESS_M,
                    WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(default_wall_material.clone()),
                Transform::from_xyz(0.0, -table_depth_m / 2.0 - WALL_THICKNESS_M / 2.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(table_width_m + 2.0 * WALL_THICKNESS_M, WALL_THICKNESS_M),
            ),
            (
                Name::from("Top Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    table_width_m + 2.0 * WALL_THICKNESS_M,
                    WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(default_wall_material.clone()),
                Transform::from_xyz(0.0, table_depth_m / 2.0 + WALL_THICKNESS_M / 2.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(table_width_m + 2.0 * WALL_THICKNESS_M, WALL_THICKNESS_M),
            ),
            (
                Name::from("Left Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    WALL_THICKNESS_M,
                    table_depth_m + 2.0 * WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(default_wall_material.clone()),
                Transform::from_xyz(-table_width_m / 2.0 - WALL_THICKNESS_M / 2.0, 0.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(WALL_THICKNESS_M, table_depth_m + 2.0 * WALL_THICKNESS_M),
            ),
            (
                Name::from("Right Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    WALL_THICKNESS_M,
                    table_depth_m + 2.0 * WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(default_wall_material.clone()),
                Transform::from_xyz(table_width_m / 2.0 + WALL_THICKNESS_M / 2.0, 0.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(WALL_THICKNESS_M, table_depth_m + 2.0 * WALL_THICKNESS_M),
            ),
        ],
    )
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
struct Table;

/// The currently loaded table; inserted by the loading screen once a table is
/// chosen, and read by everything that builds the playfield.
#[derive(Resource, Clone)]
pub struct TableAssets {
    pub(crate) file_name: String,
    pub(crate) vpx: Handle<VpxAsset>,
}
