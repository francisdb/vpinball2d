//! Mouse ball control for development purposes

use crate::pinball::ball::Ball;
use crate::{AppSystems, PausableSystems};

use avian2d::prelude::*;
use bevy::app::{App, Update};
use bevy::camera::{Camera, Camera2d};
use bevy::input::ButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

const BALL_CONTROL_STRENGTH: f32 = 5.0;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        mouse_ball_control
            .in_set(AppSystems::RecordInput)
            .in_set(PausableSystems),
    );
}

fn mouse_ball_control(
    mut commands: Commands,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    gravity: Res<Gravity>,
    mut ball_query: Query<(Entity, &Transform, &Mass, &mut LinearVelocity), With<Ball>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    if mouse_buttons.pressed(MouseButton::Left) {
        if let Some((camera, camera_transform)) = camera_query.single().ok()
            && let Some(world_position) = window
                .cursor_position()
                .and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok())
                .map(|ray| ray.origin.truncate())
        {
            for (entity, transform, mass, mut velocity) in ball_query.iter_mut() {
                let ball_pos = transform.translation.truncate();
                let direction = (world_position - ball_pos).normalize_or_zero();
                let distance = world_position.distance(ball_pos);
                // adjust ball velocity towards the mouse position
                velocity.0 = direction * distance * BALL_CONTROL_STRENGTH;
                // cancel gravity
                commands
                    .entity(entity)
                    .insert(ConstantForce(-gravity.0 * mass.0));
            }
        }
    } else {
        for (entity, _transform, _mass, _velocity) in ball_query.iter_mut() {
            // keep velocity so we can sling the ball around but cancel the anti-gravity force
            commands.entity(entity).remove::<ConstantForce>();
        }
    }
}
