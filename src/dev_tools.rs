//! Development tools for the game. This plugin is only enabled in dev builds.

use avian2d::prelude::{Collider, PhysicsDebugPlugin, PhysicsGizmos};
use bevy::color::palettes::css::LIME;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::{
    dev_tools::states::log_transitions, input::common_conditions::input_just_pressed, prelude::*,
};

use crate::pinball::playfield::Playfield;
use crate::screens::Screen;

pub(super) fn plugin(app: &mut App) {
    info!("Enabling development tools plugin");
    // Log `Screen` state transitions.
    app.add_systems(Update, log_transitions::<Screen>);

    // avian collider debug rendering (gizmos); configured off by default in `apply_collider_view`.
    app.add_plugins(PhysicsDebugPlugin);
    app.init_resource::<ColliderView>();
    app.add_systems(Startup, apply_collider_view);

    // Toggle the debug overlay for UI.
    app.add_systems(
        Update,
        toggle_debug_ui.run_if(input_just_pressed(TOGGLE_KEY)),
    );

    // Cycle collider visualisation modes: normal -> hide non-colliders -> collider wireframe only.
    app.add_systems(
        Update,
        (cycle_collider_view, apply_collider_view)
            .chain()
            .run_if(input_just_pressed(COLLIDER_VIEW_KEY)),
    );

    // Toggle slow motion to inspect the physics.
    app.add_systems(
        Update,
        toggle_slow_motion.run_if(input_just_pressed(SLOW_MOTION_KEY)),
    );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;
const COLLIDER_VIEW_KEY: KeyCode = KeyCode::KeyH;
const SLOW_MOTION_KEY: KeyCode = KeyCode::KeyS;
/// Time scale applied while slow motion is on (1/5 of real time).
const SLOW_MOTION_SPEED: f32 = 0.2;

/// Collider visualisation mode, cycled with [`COLLIDER_VIEW_KEY`].
#[derive(Resource, Default, Clone, Copy)]
enum ColliderView {
    /// Everything rendered normally, no collider wireframe.
    #[default]
    Normal,
    /// Hide meshes that have no collider, keeping the collider meshes (and playfield backdrop).
    HideNonColliders,
    /// Hide all meshes and draw only the collider wireframe (over the playfield backdrop).
    WireframeOnly,
}

fn toggle_debug_ui(mut options: ResMut<UiDebugOptions>) {
    options.toggle();
}

fn cycle_collider_view(mut view: ResMut<ColliderView>) {
    *view = match *view {
        ColliderView::Normal => ColliderView::HideNonColliders,
        ColliderView::HideNonColliders => ColliderView::WireframeOnly,
        ColliderView::WireframeOnly => ColliderView::Normal,
    };
}

/// Apply the current [`ColliderView`]: mesh visibility + avian collider wireframe.
///
/// The [`Playfield`] is kept as a backdrop so the colliders stay in context.
fn apply_collider_view(
    view: Res<ColliderView>,
    mut store: ResMut<GizmoConfigStore>,
    mut non_collider: Query<&mut Visibility, (With<Mesh2d>, Without<Collider>, Without<Playfield>)>,
) {
    let (non_collider_vis, wireframe, hide_collider_meshes, label) = match *view {
        ColliderView::Normal => (Visibility::Inherited, None, false, "normal"),
        ColliderView::HideNonColliders => (Visibility::Hidden, None, false, "hide non-colliders"),
        ColliderView::WireframeOnly => (
            Visibility::Hidden,
            Some(Color::from(LIME)),
            true,
            "collider wireframe only",
        ),
    };
    let (config, gizmos) = store.config_mut::<PhysicsGizmos>();
    config.enabled = true;
    // Start from "all options off" so avian does not draw body-axis crosshairs/AABBs/contacts; then
    // turn on only the collider wireframe. `collider_color = None` keeps it off; `hide_meshes` hides
    // meshes of entities that *have* a collider (we hide the rest ourselves).
    *gizmos = PhysicsGizmos::none();
    gizmos.collider_color = wireframe;
    gizmos.hide_meshes = hide_collider_meshes;
    for mut visibility in &mut non_collider {
        *visibility = non_collider_vis;
    }
    info!("Collider view: {label}");
}

/// Toggle slow motion by scaling virtual time, which the physics (and everything else on the
/// virtual clock) runs on: 1/5 of real time, or back to real time.
fn toggle_slow_motion(mut slow: Local<bool>, mut time: ResMut<Time<Virtual>>) {
    *slow = !*slow;
    time.set_relative_speed(if *slow { SLOW_MOTION_SPEED } else { 1.0 });
    info!("Slow motion {}", if *slow { "on (1/5)" } else { "off" });
}
