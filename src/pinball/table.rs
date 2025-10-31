//! Table-specific behavior.

use crate::asset_tracking::LoadResource;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use bevy::window::PrimaryWindow;
use vpin::vpx::vpu_to_m;

// Typical pinball wall thickness is 3/4 inch = 19.05mm
const WALL_THICKNESS_M: f32 = 0.01905;

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
    window: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&Camera, &Projection), With<Camera2d>>,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let playfield_image = vpx_asset
        .named_images
        .get(vpx_asset.raw.gamedata.image.as_str())
        .unwrap();
    let playfield_material = materials.add(ColorMaterial {
        //color: css::WHITE.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: Some(playfield_image.clone()),
        ..default()
    });
    let default_wall_material = materials.add(ColorMaterial {
        color: css::BLACK.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture: None,
        ..default()
    });

    // add a backdrop
    let backglass_material = match vpx_asset.raw.gamedata.backglass_image_full_desktop.as_str() {
        "" => materials.add(ColorMaterial {
            color: css::WHITE.with_alpha(0.0).into(),
            alpha_mode: AlphaMode2d::Blend,
            texture: None,
            ..default()
        }),
        _ => {
            let backglass_image = vpx_asset
                .named_images
                .get(vpx_asset.raw.gamedata.backglass_image_full_desktop.as_str())
                .unwrap();
            materials.add(ColorMaterial {
                //color: css::WHITE.into(),
                alpha_mode: AlphaMode2d::Opaque,
                texture: Some(backglass_image.clone()),
                ..default()
            })
        }
    };

    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);

    let window = window.single().unwrap();
    let (camera, proj) = camera_q.single().unwrap();
    let ortho = match proj {
        Projection::Orthographic(ortho) => ortho,
        _ => panic!("Expected orthographic camera"),
    };

    // Backglass fills the entire window
    let backglass_width = ortho.area.max.x - ortho.area.min.x;
    let backglass_height = table_depth_m;
    let backglass_mesh = Mesh::from(Rectangle::new(backglass_width, backglass_height));

    // TODO if there is a primitive named "playfield_mesh" we should use that mesh instead.
    //   eg this is used where the playfield has holes. Not sure this makes sense for 2D though.
    let playfield_mesh = meshes.add(Rectangle::new(table_width_m, table_depth_m));

    (
        Table,
        Name::from("Table"),
        Transform::default(),
        Visibility::default(),
        children![
            (
                Name::from("Backglass"),
                Mesh2d(meshes.add(backglass_mesh)),
                MeshMaterial2d(backglass_material),
                Transform::from_xyz(0.0, 0.0, -20.0)
            ),
            (
                Name::from("Origin"),
                Mesh2d(meshes.add(Mesh::from(Circle::new(0.01)))),
                MeshMaterial2d(materials.add(Color::from(css::RED))),
                Transform::from_xyz(0.0, 0.0, 1.0),
            ),
            (
                Name::from("Playfield"),
                Mesh2d(playfield_mesh),
                MeshMaterial2d(playfield_material),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ),
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

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct TableAssets {
    pub(crate) file_name: String,
    #[dependency]
    pub(crate) vpx: Handle<VpxAsset>,
}

impl FromWorld for TableAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        let file_name = "exampleTable.vpx".to_string();
        //let file_name = "North Pole (Playmatic 1967) v600.vpx";
        Self {
            file_name: file_name.to_string(),
            vpx: assets.load(file_name),
        }
    }
}
