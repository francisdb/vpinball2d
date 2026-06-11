use avian2d::prelude::{Collider, CollisionEventsEnabled, RigidBody, Sensor};
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

#[derive(Component)]
pub struct Trigger {
    #[allow(dead_code)]
    pub name: String,
}

pub(super) fn spawn_trigger(
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    trigger: &vpx::gameitem::trigger::Trigger,
) {
    // TODO triggers in case the shape is None have a custom polygon shape
    // TODO we may want to draw the wire, button or star shape depending on the
    //   trigger type; for now triggers are invisible (the playfield art usually
    //   shows them) and only visible in the dev collider view.
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
        // physics only: no visual, the collider shows in the dev collider view
        CollisionEventsEnabled,
        RigidBody::Static,
        Collider::circle(radius),
        Sensor,
    ));
}

// TODO handle ball-trigger collisions
