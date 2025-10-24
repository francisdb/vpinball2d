// Support configuring Bevy lints within code.
#![cfg_attr(bevy_lint, feature(register_tool), register_tool(bevy))]
// Disable console on Windows for non-dev builds.
#![cfg_attr(not(feature = "dev"), windows_subsystem = "windows")]

mod asset_tracking;
mod audio;
#[cfg(feature = "dev")]
mod dev_tools;
mod menus;
mod pinball;
mod screens;
mod theme;
mod vpx;

mod diagnostics;

use crate::diagnostics::DiagnosticsPlugin;
use crate::pinball::table::{TABLE_DEPTH_VPU, TABLE_WIDTH_VPU};
use crate::vpx::VpxPlugin;
use avian2d::PhysicsPlugins;
use avian2d::math::Vector;
use avian2d::prelude::*;
use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::{asset::AssetMetaCheck, prelude::*};
use vpin::vpx::vpu_to_m;
// use bevy_inspector_egui::bevy_egui::EguiPlugin;
// use bevy_inspector_egui::quick::WorldInspectorPlugin;

/// Spatial audio uses the distance to attenuate the sound volume. In 2D with the default camera,
/// 1 pixel is 1 unit of distance, so we use a scale so that 100 pixels is 1 unit of distance for
/// audio.
const AUDIO_SCALE: f32 = 1.0;

fn main() -> AppExit {
    App::new().add_plugins(AppPlugin).run()
}

pub struct AppPlugin;

impl Plugin for AppPlugin {
    fn build(&self, app: &mut App) {
        // Add Bevy plugins.
        app.add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    // Wasm builds will check for meta files (that don't exist) if this isn't set.
                    // This causes errors and even panics on web build on itch.
                    // See https://github.com/bevyengine/bevy_github_ci_template/issues/48.
                    meta_check: AssetMetaCheck::Never,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Window {
                        title: "VPinball2D".to_string(),
                        fit_canvas_to_parent: true,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(AudioPlugin {
                    default_spatial_scale: SpatialScale::new_2d(AUDIO_SCALE),
                    ..default()
                }),
            // One unit in bevy is one meter
            // However I have the impression that this should be adjusted to the average object size
            // in the scene? So we set it to 0.1 to have more reasonable values for debug rendering
            PhysicsPlugins::default().with_length_unit(0.1),
            // DiagnosticsPlugin,
        ));
        // gravity of approx. 9.81 m/s² but with a table at 7° angle
        app.insert_resource(Gravity(Vector::NEG_Y * 9.81 * 0.12192));
        // to improve physics stability
        app.insert_resource(SubstepCount(50));

        // #[cfg(feature = "dev")]
        // app.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::new()));

        // Add other plugins.
        app.add_plugins((
            VpxPlugin,
            asset_tracking::plugin,
            audio::plugin,
            pinball::plugin,
            #[cfg(feature = "dev")]
            dev_tools::plugin,
            menus::plugin,
            screens::plugin,
            theme::plugin,
        ));

        // Order new `AppSystems` variants by adding them here:
        app.configure_sets(
            Update,
            (
                AppSystems::TickTimers,
                AppSystems::RecordInput,
                AppSystems::Update,
            )
                .chain(),
        );

        // Set up the `Pause` state.
        app.init_state::<Pause>();
        app.configure_sets(Update, PausableSystems.run_if(in_state(Pause(false))));

        // Spawn the main camera.
        app.add_systems(Startup, spawn_camera);
    }
}

/// High-level groupings of systems for the app in the `Update` schedule.
/// When adding a new variant, make sure to order it in the `configure_sets`
/// call above.
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord)]
enum AppSystems {
    /// Tick timers.
    TickTimers,
    /// Record player input.
    RecordInput,
    /// Do everything else (consider splitting this into further variants).
    Update,
}

/// Whether or not the game is paused.
#[derive(States, Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
struct Pause(pub bool);

/// A system set for systems that shouldn't run while the game is paused.
#[derive(SystemSet, Copy, Clone, Eq, PartialEq, Hash, Debug)]
struct PausableSystems;

/// Spawn the main 2D camera with orthographic projection that fits the table.
///
/// This does not match the original VPinball coordinate system as there the Y axis is
/// inverted compared to Bevy's coordinate system.
/// Further the origin is at the top-left of the table in VPinball, while we use the
/// center of the table as origin in Bevy.
fn spawn_camera(mut commands: Commands) {
    let table_width_m = vpu_to_m(TABLE_WIDTH_VPU);
    let table_depth_m = vpu_to_m(TABLE_DEPTH_VPU);
    commands.spawn((
        Name::new("Camera"),
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMin {
                min_height: table_depth_m,
                min_width: table_width_m,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
}
