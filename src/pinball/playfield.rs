//! The playfield: the flat table surface the ball rolls on.
//!
//! In this 2D top-down view the playfield is purely visual - gravity keeps the ball in
//! plane, so there is no floor collider. It carries the [`Playfield`] marker so tooling
//! can treat it as the backdrop (e.g. keep it visible when hiding non-collider meshes).

use crate::vpx::VpxAsset;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::units::vpu_to_m;

/// Marker for the playfield surface entity.
#[derive(Component, Debug, Clone, Copy, Default, Reflect)]
#[reflect(Component)]
pub struct Playfield;

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
