use crate::vpx::VpxAsset;
use avian2d::prelude::{Collider, CollisionEventsEnabled, RigidBody, Sensor};
use bevy::asset::Assets;
use bevy::color::palettes::css;
use bevy::color::{Color, Srgba};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::{
    Annulus, ChildOf, ColorMaterial, Component, MeshMaterial2d, Name, ResMut, Transform,
};
use vpin::vpx;
use vpin::vpx::vpu_to_m;

const RUBER_COLOR: Srgba = css::WHITE;

#[derive(Component)]
pub struct Rubber {
    pub name: String,
}

pub(super) fn spawn_rubber(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    rubber: &vpx::gameitem::rubber::Rubber,
    vpx_asset: &VpxAsset,
) {
    // a rubber is presented by a ring shape formed by the rubber.drag_points
    // with the thickness rubber.thickness

    let mesh_handle = vpx_asset
        .named_meshes
        .get(VpxAsset::rubber_mesh_sub_path(&rubber.name).as_str())
        .unwrap();

    parent.spawn((
        Rubber {
            name: rubber.name.clone(),
        },
        Name::from(format!("RUbber {}", rubber.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x,
            vpx_to_bevy_transform.translation.y,
            0.0, // height is set in the mesh, not sure we want to keep that
        ),
        Mesh2d(mesh_handle.clone()),
        MeshMaterial2d(materials.add(Color::from(RUBER_COLOR))),
        // physics
        CollisionEventsEnabled,
        RigidBody::Static,
        // TODO proper collider shape
        Sensor,
    ));
}
