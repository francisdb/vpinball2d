use avian2d::prelude::{Collider, CollisionEventsEnabled, RigidBody, Sensor};
use bevy::color::Color;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Mesh, Mesh2d};
use bevy::prelude::*;
use bevy::prelude::{Annulus, ChildOf, ColorMaterial, MeshMaterial2d, Name, ResMut, Transform};
use vpin::vpx;
use vpin::vpx::vpu_to_m;

#[derive(Component)]
pub struct Trigger {
    #[allow(dead_code)]
    pub name: String,
}

const TRIGGER_COLOR: Srgba = css::YELLOW;

pub(super) fn spawn_trigger(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    trigger: &vpx::gameitem::trigger::Trigger,
) {
    // TODO triggers in case the shape is None have a custom polygon shape
    // TODO make the drag_points accessible in the vpin lib

    // trigger.drag_points.clone();

    // we also want to draw the wire, the button or the star shape depending on the trigger type
    let radius = vpu_to_m(trigger.radius);
    parent.spawn((
        Trigger {
            name: trigger.name.clone(),
        },
        Name::from(format!("Trigger {}", trigger.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(trigger.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(trigger.center.y),
            10.0,
        ),
        Mesh2d(meshes.add(Annulus::new(radius - 0.001, radius))),
        MeshMaterial2d(materials.add(Color::from(TRIGGER_COLOR))),
        // physics
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(radius),
        Sensor,
    ));
}

// TODO handle ball-trigger collisions
