use avian2d::prelude::{Collider, CollisionEventsEnabled, RigidBody, Sensor};
use bevy::asset::Assets;
use bevy::color::Color;
use bevy::color::Srgba;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::{
    Annulus, ChildOf, ColorMaterial, Component, MeshMaterial2d, Name, ResMut, Transform,
};
use vpin::vpx;
use vpin::vpx::vpu_to_m;

#[derive(Component)]
pub struct Kicker {
    #[allow(dead_code)]
    pub name: String,
}

const KICKER_COLOR: Srgba = css::GREEN;

pub(super) fn spawn_kicker(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    kicker: &vpx::gameitem::kicker::Kicker,
) {
    let radius = vpu_to_m(kicker.radius);
    parent.spawn((
        Kicker {
            name: kicker.name.clone(),
        },
        Name::from(format!("Kicker {}", kicker.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(kicker.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(kicker.center.y),
            10.0,
        ),
        Mesh2d(meshes.add(Annulus::new(radius - 0.001, radius))),
        MeshMaterial2d(materials.add(Color::from(KICKER_COLOR))),
        // physics
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(radius),
        Sensor,
    ));
}
