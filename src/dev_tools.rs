//! Development tools for the game. This plugin is only enabled in dev builds.

use avian2d::collision::CollisionDiagnostics;
use avian2d::diagnostics::{
    PhysicsEntityDiagnostics, PhysicsEntityDiagnosticsPlugin, PhysicsTotalDiagnostics,
    PhysicsTotalDiagnosticsPlugin,
};
use avian2d::dynamics::solver::SolverDiagnostics;
use avian2d::prelude::{
    Collider, PhysicsDebugPlugin, PhysicsDiagnosticsUiPlugin, PhysicsDiagnosticsUiSettings,
    PhysicsGizmos,
};
use bevy::color::palettes::css::LIME;
use bevy::dev_tools::diagnostics_overlay::{DiagnosticsOverlay, DiagnosticsOverlayPlugin};
use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
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

    // Two physics-aware diagnostics overlays, to compare:
    // - F3: bevy 0.19's draggable `DiagnosticsOverlay` (fps + selected avian physics
    //   stats), reading the shared `DiagnosticsStore`.
    // - F4: avian's own `PhysicsDiagnosticsUiPlugin` (richer physics panel + graphs).
    // FrameTimeDiagnosticsPlugin supplies fps; the two avian *Diagnostics plugins
    // supply step time + body/collider counts (collision/solver auto-register with
    // the running physics).
    app.add_plugins((
        FrameTimeDiagnosticsPlugin::default(),
        DiagnosticsOverlayPlugin,
        PhysicsTotalDiagnosticsPlugin,
        PhysicsEntityDiagnosticsPlugin,
        PhysicsDiagnosticsUiPlugin,
    ));
    app.add_systems(
        Update,
        (
            toggle_diagnostics_overlay.run_if(input_just_pressed(DIAGNOSTICS_KEY)),
            toggle_physics_ui.run_if(input_just_pressed(PHYSICS_UI_KEY)),
        ),
    );

    // avian's UI defaults to enabled; start it hidden so dev launches are clean
    // (F4 shows it). Our overlay stays unspawned until F3.
    app.add_systems(
        Startup,
        |mut settings: ResMut<PhysicsDiagnosticsUiSettings>| {
            settings.enabled = false;
        },
    );
}

const TOGGLE_KEY: KeyCode = KeyCode::Backquote;
const COLLIDER_VIEW_KEY: KeyCode = KeyCode::KeyH;
const SLOW_MOTION_KEY: KeyCode = KeyCode::KeyS;
/// Toggle our bevy diagnostics overlay window (fps + physics stats).
const DIAGNOSTICS_KEY: KeyCode = KeyCode::F3;
/// Toggle avian's own physics diagnostics UI.
const PHYSICS_UI_KEY: KeyCode = KeyCode::F4;
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

fn toggle_debug_ui(mut options: ResMut<GlobalUiDebugOptions>) {
    options.toggle();
}

/// Our overlay's contents: fps plus a few avian physics stats, all read from the
/// shared `DiagnosticsStore`. Times are in ms; counts are live values.
fn diagnostics_overlay() -> DiagnosticsOverlay {
    // avian's path constants are `&'static DiagnosticPath`; deref + clone to the
    // owned `DiagnosticPath` the overlay item expects. bevy's FPS const is owned.
    DiagnosticsOverlay::new(
        "Diagnostics",
        vec![
            FrameTimeDiagnosticsPlugin::FPS.into(),
            FrameTimeDiagnosticsPlugin::FRAME_TIME.into(),
            (*PhysicsTotalDiagnostics::STEP_TIME).clone().into(),
            (*CollisionDiagnostics::CONTACT_COUNT).clone().into(),
            (*SolverDiagnostics::SOLVE_CONSTRAINTS).clone().into(),
            (*PhysicsEntityDiagnostics::DYNAMIC_BODY_COUNT)
                .clone()
                .into(),
            (*PhysicsEntityDiagnostics::COLLIDER_COUNT).clone().into(),
        ],
    )
}

/// Toggle avian's own physics diagnostics UI via its settings resource.
fn toggle_physics_ui(mut settings: ResMut<PhysicsDiagnosticsUiSettings>) {
    settings.enabled = !settings.enabled;
}

/// Toggle our diagnostics overlay window: spawn it when absent, despawn it when
/// present. The plugin reparents it under its own overlay plane.
fn toggle_diagnostics_overlay(
    mut commands: Commands,
    existing: Query<Entity, With<DiagnosticsOverlay>>,
) {
    if existing.is_empty() {
        commands.spawn(diagnostics_overlay());
    } else {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
    }
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
