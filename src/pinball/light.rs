use bevy::asset::Assets;
use bevy::color::{Color, Srgba};
use bevy::ecs::children;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::vpu_to_m;

#[derive(Component)]
pub struct Light {
    #[allow(dead_code)]
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
    let light_color = Srgba::rgb_u8(light.color.r, light.color.g, light.color.b).with_alpha(0.6);
    let light_falloff_color = light_color.clone().with_alpha(0.1);
    // TODO check what the correct default is in vpinball
    const DEFAULT_LIGHT_HEIGHT: f32 = 0.01;
    parent.spawn((
        Light {
            name: light.name.clone(),
        },
        Name::from(format!("Light {}", light.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(light.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(light.center.y),
            vpu_to_m(light.height.unwrap_or(DEFAULT_LIGHT_HEIGHT)),
        ),
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(materials.add(Color::from(light_color).with_alpha(0.5))),
        children![(
            Mesh2d(meshes.add(Circle::new(falloff_radius))),
            MeshMaterial2d(materials.add(Color::from(light_falloff_color).with_alpha(0.1))),
            Transform::from_xyz(0.0, 0.0, -0.001)
        )],
    ));
}
