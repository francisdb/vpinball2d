use bevy::asset::Assets;
use bevy::color::palettes::css;
use bevy::color::{Color, Srgba};
use bevy::ecs::children;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::vpu_to_m;

const LIGHT_COLOR: Srgba = css::YELLOW;
const LIGHT_FALLOFF_COLOR: Srgba = css::YELLOW;

#[derive(Component)]
pub struct Light {
    pub name: String,
}

pub(super) fn spawn_light(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    light: &vpx::gameitem::light::Light,
) {
    let radius = vpu_to_m(light.mesh_radius);
    let falloff_radius = vpu_to_m(light.falloff_radius);
    parent.spawn((
        Light {
            name: light.name.clone(),
        },
        Name::from(format!("Light {}", light.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(light.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(light.center.y),
            vpu_to_m(light.height.unwrap()),
        ),
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(materials.add(Color::from(LIGHT_COLOR).with_alpha(0.5))),
        children![(
            Mesh2d(meshes.add(Circle::new(falloff_radius))),
            MeshMaterial2d(materials.add(Color::from(LIGHT_FALLOFF_COLOR).with_alpha(0.1))),
            Transform::from_xyz(0.0, 0.0, -0.001)
        )],
    ));
}
