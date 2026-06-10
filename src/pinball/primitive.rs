//! Visible primitives, rendered as their top-down projection.
//!
//! VPinball primitives are baked 3D meshes. Here we draw only the upward-facing part (see
//! `crate::vpx::primitive_mesh`) with the primitive's image/material, at the mesh's world
//! height for depth ordering. Primitives are decorative in this 2D world; their collision,
//! when any, comes from separate wall/ramp items, so no collider is added here.

use crate::vpx::VpxAsset;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::primitive;

pub(super) fn plugin(_app: &mut App) {}

#[derive(Component)]
pub struct Primitive {
    /// The vpx primitive name; kept for debugging and tooling.
    #[allow(dead_code)]
    pub name: String,
}

pub(super) fn spawn_primitive(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    primitive: &primitive::Primitive,
) {
    // Flipper bats are textured primitives the flipper draws rotating with it; skip them
    // here so they are not also drawn statically.
    if crate::pinball::flipper::is_flipper_bat(&vpx_asset.raw.gameitems, primitive) {
        return;
    }

    // Bumper caps are textured discs the bumper draws flat (a cap's dome flattens to a
    // distorted blob top-down); skip them here so they are not drawn twice.
    if crate::pinball::bumper::is_bumper_cap(&vpx_asset.raw.gameitems, primitive) {
        return;
    }

    // A top-down mesh is only generated for visible primitives with upward-facing geometry.
    let Some(mesh_handle) = vpx_asset
        .named_meshes
        .get(VpxAsset::primitive_mesh_sub_path(&primitive.name).as_str())
    else {
        return;
    };

    // Colour/transparency from the primitive material, mirroring walls and ramps (base
    // colour tinted, alpha blending when the material opacity is active or the image has
    // an alpha channel - the latter matters for cut-out decals like bumper caps).
    let material = vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == primitive.material);
    let texture = vpx_asset.image(primitive.image.as_str()).cloned();
    let texture_has_alpha = !vpx_asset
        .raw
        .images
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(primitive.image.as_str()))
        .and_then(|i| i.is_opaque)
        .unwrap_or(true);
    let (color, alpha_mode) = if let Some(mat) = material {
        let alpha = if mat.opacity_active { mat.opacity } else { 1.0 };
        let blend = texture_has_alpha || (mat.opacity_active && alpha < 0.999);
        let color = Srgba {
            alpha,
            ..Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
        };
        (
            color,
            if blend {
                AlphaMode2d::Blend
            } else {
                AlphaMode2d::Opaque
            },
        )
    } else if texture_has_alpha {
        (css::WHITE, AlphaMode2d::Blend)
    } else {
        (css::WHITE, AlphaMode2d::Opaque)
    };

    let material = materials.add(ColorMaterial {
        color: color.into(),
        alpha_mode,
        texture,
        ..default()
    });

    parent.spawn((
        Name::from(format!("Primitive {}", primitive.name)),
        Primitive {
            name: primitive.name.clone(),
        },
        Mesh2d(mesh_handle.clone()),
        MeshMaterial2d(material),
        vpx_to_bevy_transform,
    ));
}
