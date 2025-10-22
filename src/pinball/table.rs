//! Table-specific behavior.

use crate::asset_tracking::LoadResource;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
// The vpinball demo table is 2162mm deep and 952mm wide.
// TODO: get that info from the vpx file directly.

const TABLE_WIDTH_M: f32 = 0.952;
const TABLE_DEPTH_M: f32 = 2.162;
// Typical pinball wall thickness is 3/4 inch = 19.05mm
const WALL_THICKNESS_M: f32 = 0.01905;

pub(crate) const FULL_WIDTH_M: f32 = TABLE_WIDTH_M + 2.0 * WALL_THICKNESS_M;
pub(crate) const FULL_DEPTH_M: f32 = TABLE_DEPTH_M + 2.0 * WALL_THICKNESS_M;

pub(super) fn plugin(app: &mut App) {
    app.load_resource::<TableAssets>();

    // // Record directional input as movement controls.
    // app.add_systems(
    //     Update,
    //     record_player_directional_input
    //         .in_set(AppSystems::RecordInput)
    //         .in_set(PausableSystems),
    // );
}

/// The pinball table
pub(crate) fn table(
    // max_speed: f32,
    table_assets: &TableAssets,
    // texture_atlas_layouts: &mut Assets<TextureAtlasLayout>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    assets_vpx: &Res<Assets<VpxAsset>>,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let playfield_image = vpx_asset
        .named_images
        .get(vpx_asset.raw.gamedata.image.as_str())
        .unwrap();

    let material = materials.add(ColorMaterial {
        //color: css::WHITE.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: Some(playfield_image.clone()),
        ..default()
    });
    let wall_material = materials.add(ColorMaterial {
        color: css::BLACK.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });

    // TODO look into using a compound collider for better performance
    // Collider::compound(vec![

    (
        Table,
        Name::from("Table"),
        Transform::default(),
        Visibility::default(),
        children![
            (
                Name::from("Origin"),
                Mesh2d(meshes.add(Mesh::from(Circle::new(0.01)))),
                MeshMaterial2d(materials.add(Color::from(css::RED))),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ),
            (
                Name::from("Table Floor"),
                Mesh2d(meshes.add(Rectangle::new(TABLE_WIDTH_M, TABLE_DEPTH_M))),
                MeshMaterial2d(material),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ),
            (
                Name::from("Bottom Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    TABLE_WIDTH_M + 2.0 * WALL_THICKNESS_M,
                    WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(wall_material.clone()),
                Transform::from_xyz(0.0, -TABLE_DEPTH_M / 2.0 - WALL_THICKNESS_M / 2.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(TABLE_WIDTH_M + 2.0 * WALL_THICKNESS_M, WALL_THICKNESS_M),
            ),
            (
                Name::from("Top Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    TABLE_WIDTH_M + 2.0 * WALL_THICKNESS_M,
                    WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(wall_material.clone()),
                Transform::from_xyz(0.0, TABLE_DEPTH_M / 2.0 + WALL_THICKNESS_M / 2.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(TABLE_WIDTH_M + 2.0 * WALL_THICKNESS_M, WALL_THICKNESS_M),
            ),
            (
                Name::from("Left Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    WALL_THICKNESS_M,
                    TABLE_DEPTH_M + 2.0 * WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(wall_material.clone()),
                Transform::from_xyz(-TABLE_WIDTH_M / 2.0 - WALL_THICKNESS_M / 2.0, 0.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(WALL_THICKNESS_M, TABLE_DEPTH_M + 2.0 * WALL_THICKNESS_M),
            ),
            (
                Name::from("Right Wall"),
                Mesh2d(meshes.add(Rectangle::new(
                    WALL_THICKNESS_M,
                    TABLE_DEPTH_M + 2.0 * WALL_THICKNESS_M,
                ))),
                MeshMaterial2d(wall_material),
                Transform::from_xyz(TABLE_WIDTH_M / 2.0 + WALL_THICKNESS_M / 2.0, 0.0, 0.1),
                RigidBody::Static,
                Collider::rectangle(WALL_THICKNESS_M, TABLE_DEPTH_M + 2.0 * WALL_THICKNESS_M),
            ),
        ],
    )
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Component)]
struct Table;

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct TableAssets {
    #[dependency]
    pub(crate) vpx: Handle<VpxAsset>,
}

impl FromWorld for TableAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            vpx: assets.load("exampleTable.vpx"),
        }
    }
}
