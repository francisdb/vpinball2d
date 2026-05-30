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
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;
const HIDE_NON_COLLIDERS_KEY: KeyCode = KeyCode::KeyH;

fn toggle_debug_ui(mut options: ResMut<UiDebugOptions>) {
    options.toggle();
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
