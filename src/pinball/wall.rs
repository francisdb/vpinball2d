use crate::pinball::ball::BALL_RADIUS_M;
use crate::vpx::VpxAsset;
use avian2d::math::Vector;

use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::wall::Wall;

pub(super) fn spawn_wall(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    wall: &Wall,
) {
    let name = Name::from(format!("Wall {}", wall.name));
    let mesh_handle = vpx_asset
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
    if wall.is_collidable && wall.height_bottom < BALL_RADIUS_M * 2.0 {
        let mesh = meshes.get(mesh_handle).unwrap();
        let collider = mesh_collider(mesh);
        parent.spawn((
            name,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            vpx_to_bevy_transform,
            RigidBody::Static,
            Restitution::from(wall.elasticity),
            Friction::from(wall.friction),
            collider,
        ));
    } else {
        parent.spawn((
            name,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            vpx_to_bevy_transform,
        ));
    }
}

/// Create a polyline collider from the 2D mesh vertices
fn mesh_collider(mesh: &Mesh) -> Collider {
    let vertices: Vec<Vector> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap()
        .iter()
        .map(|v| Vector::new(v[0], v[1]))
        .collect();
    // we have to duplicate the first vertex at the end to close the loop
    let mut vertices = vertices;
    vertices.push(vertices[0]);
    Collider::polyline(vertices, None)
}
