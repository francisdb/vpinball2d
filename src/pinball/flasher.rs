//! Flashers: textured overlay polygons above the playfield.
//!
//! Only the static, non-additive image flashers render: modern tables print
//! their insert text/graphics through a full-table flasher overlay (the
//! playfield image ships with blank inserts, the lights glow underneath, and
//! the flasher draws the labels over the glow), so without it lit inserts are
//! unreadable colour blocks. Additive flashers are flash effects a script
//! drives at runtime; they are authored at full brightness and a script dims
//! or hides them, so without scripts they are skipped like other
//! script-driven state.

use crate::vpx::VpxAsset;
use crate::vpx::triangulate::triangulate_polygon;
use bevy::asset::RenderAssetUsages;
use bevy::color::Srgba;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, Mesh, Mesh2d, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::flasher::Flasher;
use vpin::vpx::gameitem::ramp_image_alignment::RampImageAlignment;
use vpin::vpx::units::vpu_to_m;

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_flasher(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    flasher: &Flasher,
    item_index: usize,
) {
    if !flasher.is_visible || flasher.add_blend {
        return;
    }
    // An image-less flasher is a runtime canvas (e.g. a DMD); nothing to show.
    let Some(texture) = vpx_asset.image(flasher.image_a.as_str()).cloned() else {
        return;
    };
    if flasher.drag_points.len() < 3 {
        return;
    }
    let smoothed = vpin::vpx::mesh::smooth_drag_points_2d(&flasher.drag_points, 4.0, true);
    let positions: Vec<[f32; 3]> = smoothed
        .iter()
        .map(|(x, y)| [vpu_to_m(*x), -vpu_to_m(*y), 0.0])
        .collect();
    let gamedata = &vpx_asset.raw.gamedata;
    let table_size = Vec2::new(
        (gamedata.right - gamedata.left).max(1.0),
        (gamedata.bottom - gamedata.top).max(1.0),
    );
    let min = smoothed
        .iter()
        .fold(Vec2::MAX, |m, (x, y)| m.min(Vec2::new(*x, *y)));
    let max = smoothed
        .iter()
        .fold(Vec2::MIN, |m, (x, y)| m.max(Vec2::new(*x, *y)));
    let extent = (max - min).max(Vec2::ONE);
    let uvs: Vec<[f32; 2]> = smoothed
        .iter()
        .map(|(x, y)| match flasher.image_alignment {
            // World: UVs in table space, like the wall-top auto coordinates.
            RampImageAlignment::World | RampImageAlignment::Unknown => {
                [x / table_size.x, y / table_size.y]
            }
            // Wrap: the image spans the flasher's bounding box.
            RampImageAlignment::Wrap => [(x - min.x) / extent.x, (y - min.y) / extent.y],
        })
        .collect();
    let outline: Vec<Vec2> = positions.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    let indices = triangulate_polygon(&outline);
    if indices.is_empty() {
        return;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    // The texture tinted by the flasher colour, faded by the alpha percentage,
    // alpha blended over whatever is below (vpinball's non-additive flasher path).
    let color = Srgba {
        alpha: (flasher.alpha.max(0) as f32 / 100.0).min(1.0),
        ..Srgba::rgb_u8(flasher.color.r, flasher.color.g, flasher.color.b)
    };
    parent.spawn((
        Name::from(format!("Flasher {}", flasher.name)),
        Mesh2d(meshes.add(mesh)),
        MeshMaterial2d(materials.add(ColorMaterial {
            color: color.into(),
            alpha_mode: AlphaMode2d::Blend,
            texture: Some(texture),
            ..default()
        })),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x,
            vpx_to_bevy_transform.translation.y,
            crate::pinball::layer::render_z(flasher.height, flasher.depth_bias, item_index),
        ),
    ));
}
