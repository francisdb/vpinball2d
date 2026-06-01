//! Development tools for the game. This plugin is only enabled in dev builds.

use avian2d::prelude::Collider;
use bevy::{
    dev_tools::states::log_transitions, input::common_conditions::input_just_pressed, prelude::*,
};

use crate::pinball::playfield::Playfield;
use crate::screens::Screen;

pub(super) fn plugin(app: &mut App) {
    info!("Enabling development tools plugin");
    // Log `Screen` state transitions.
    app.add_systems(Update, log_transitions::<Screen>);

    // Toggle the debug overlay for UI.
    app.add_systems(
        Update,
        toggle_debug_ui.run_if(input_just_pressed(TOGGLE_KEY)),
    );

    // Toggle hiding everything that has no collider, to see the collision geometry.
    app.add_systems(
        Update,
        toggle_non_collider_visibility.run_if(input_just_pressed(HIDE_NON_COLLIDERS_KEY)),
    );

    // Toggle slow motion to inspect the physics.
    app.add_systems(
        Update,
        toggle_slow_motion.run_if(input_just_pressed(SLOW_MOTION_KEY)),
    );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;
const HIDE_NON_COLLIDERS_KEY: KeyCode = KeyCode::KeyH;
const SLOW_MOTION_KEY: KeyCode = KeyCode::KeyS;
/// Time scale applied while slow motion is on (1/5 of real time).
const SLOW_MOTION_SPEED: f32 = 0.2;

fn toggle_debug_ui(mut options: ResMut<UiDebugOptions>) {
    options.toggle();
}

/// Toggle slow motion by scaling virtual time, which the physics (and everything else on the
/// virtual clock) runs on: 1/5 of real time, or back to real time.
fn toggle_slow_motion(mut slow: Local<bool>, mut time: ResMut<Time<Virtual>>) {
    *slow = !*slow;
    time.set_relative_speed(if *slow { SLOW_MOTION_SPEED } else { 1.0 });
    info!("Slow motion {}", if *slow { "on (1/5)" } else { "off" });
}

/// Hide/show every mesh that has no collider so only the collision geometry remains.
/// The [`Playfield`] is kept as a backdrop so the colliders stay in context.
fn toggle_non_collider_visibility(
    mut hidden: Local<bool>,
    mut meshes: Query<&mut Visibility, (With<Mesh2d>, Without<Collider>, Without<Playfield>)>,
) {
    *hidden = !*hidden;
    let target = if *hidden {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut visibility in &mut meshes {
        *visibility = target;
    }
    info!("Hiding non-collider meshes: {}", *hidden);
}
