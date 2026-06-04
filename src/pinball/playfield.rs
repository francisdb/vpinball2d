//! The playfield: the flat table surface the ball rolls on.
//!
//! In this 2D top-down view the playfield is purely visual - gravity keeps the ball in
//! plane, so there is no floor collider. It carries the [`Playfield`] marker so tooling
//! can treat it as the backdrop (e.g. keep it visible when hiding non-collider meshes).

use crate::screens::Screen;
use crate::vpx::VpxAsset;
use bevy::asset::AssetId;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::units::vpu_to_m;

/// Marker for the playfield surface entity.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct Playfield;

/// Default brightness multiplier for the playfield image. Dimming it lets the
/// light glows read as actual light instead of washing out against bright art.
const PLAYFIELD_BRIGHTNESS: f32 = 0.5;

/// Brightness multiplier applied to every non-playfield object material, so table
/// objects sit a touch above the dimmed playfield without popping at full
/// brightness against it. Lights use a separate material type and are unaffected.
const OBJECT_BRIGHTNESS: f32 = 0.8;

/// Tracks which object materials have already been dimmed, so shared materials are
/// dimmed exactly once and re-entry to gameplay re-dims freshly spawned materials.
#[derive(Resource, Default)]
pub(super) struct DimmedObjects(HashSet<AssetId<ColorMaterial>>);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<DimmedObjects>();
    app.add_systems(Update, dim_objects.run_if(in_state(Screen::Gameplay)));
}

/// Dims every non-playfield object material to [`OBJECT_BRIGHTNESS`]. The playfield
/// is excluded by its marker; light glows use `GlowMaterial` so they never match.
fn dim_objects(
    mut dimmed: ResMut<DimmedObjects>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    query: Query<
        &MeshMaterial2d<ColorMaterial>,
        (Added<MeshMaterial2d<ColorMaterial>>, Without<Playfield>),
    >,
) {
    for material in &query {
        // Dim each unique material once, even when shared across many entities.
        if !dimmed.0.insert(material.0.id()) {
            continue;
        }
        if let Some(color_material) = materials.get_mut(&material.0) {
            let srgba = color_material.color.to_srgba();
            color_material.color = Srgba::new(
                srgba.red * OBJECT_BRIGHTNESS,
                srgba.green * OBJECT_BRIGHTNESS,
                srgba.blue * OBJECT_BRIGHTNESS,
                srgba.alpha,
            )
            .into();
        }
    }
}

/// Build the playfield: a table-sized quad textured with the table image, marked with
/// [`Playfield`].
pub(crate) fn playfield(
    vpx_asset: &VpxAsset,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> (
    Playfield,
    Name,
    Mesh2d,
    MeshMaterial2d<ColorMaterial>,
    Transform,
) {
    let playfield_image = vpx_asset
        .named_images
        .get(vpx_asset.raw.gamedata.image.as_str())
        .unwrap();
    let material = materials.add(ColorMaterial {
        color: Color::srgb(
            PLAYFIELD_BRIGHTNESS,
            PLAYFIELD_BRIGHTNESS,
            PLAYFIELD_BRIGHTNESS,
        ),
        alpha_mode: AlphaMode2d::Opaque,
        texture: Some(playfield_image.clone()),
        ..default()
    });
    let width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);
    // TODO if there is a primitive named "playfield_mesh" we should use that mesh instead.
    //   eg this is used where the playfield has holes. Not sure this makes sense for 2D though.
    let mesh = meshes.add(Rectangle::new(width_m, depth_m));
    (
        Playfield,
        Name::from("Playfield"),
        Mesh2d(mesh),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, 0.0),
    )
}
