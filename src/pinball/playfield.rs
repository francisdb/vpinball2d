//! The playfield: the flat table surface the ball rolls on.
//!
//! In this 2D top-down view the playfield is purely visual - gravity keeps the ball in
//! plane, so there is no floor collider. It carries the [`Playfield`] marker so tooling
//! can treat it as the backdrop (e.g. keep it visible when hiding non-collider meshes).

use crate::pinball::lightmap::PlayfieldLightMaterial;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use bevy::asset::AssetId;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use vpin::vpx::units::vpu_to_m;

/// Marker for the playfield surface entity.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct Playfield;

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
        if let Some(mut color_material) = materials.get_mut(&material.0) {
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
    materials: &mut ResMut<Assets<PlayfieldLightMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    light_map: Handle<Image>,
) -> (
    Playfield,
    Name,
    Mesh2d,
    MeshMaterial2d<PlayfieldLightMaterial>,
    Transform,
) {
    // Best effort: some tables reference an image we could not load or none at
    // all. Fall back to a blank texture so the table still renders instead of
    // crashing.
    let playfield_image = match vpx_asset.image(vpx_asset.raw.gamedata.image.as_str()) {
        Some(handle) => handle.clone(),
        None => {
            warn!(
                "Playfield image '{}' not found; rendering a blank playfield",
                vpx_asset.raw.gamedata.image
            );
            images.add(blank_image())
        }
    };
    // The light map (rendered over the same rect) modulates the table image:
    // ambient where unlit, brighter where lit, darker where shadowed.
    let material = materials.add(PlayfieldLightMaterial {
        playfield: playfield_image,
        light_map,
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

/// A 1x1 white texture used as a stand-in when a table's playfield image is
/// missing or could not be loaded.
fn blank_image() -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
