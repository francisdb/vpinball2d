// Support configuring Bevy lints within code.
#![cfg_attr(bevy_lint, feature(register_tool), register_tool(bevy))]
// Disable console on Windows for non-dev builds.
#![cfg_attr(not(feature = "dev"), windows_subsystem = "windows")]

mod asset_tracking;
mod audio;
#[cfg(feature = "dev")]
mod dev_tools;
mod flexdmd;
mod pinball;
#[cfg(any(feature = "remote_control", feature = "telemetry"))]
mod play;
mod screens;
mod scripting;
mod tables;
mod theme;
mod vpx;

// mod diagnostics;

use crate::tables::{TABLES_SOURCE, TablesDir, resolve_tables};
use crate::vpx::VpxPlugin;
use avian2d::PhysicsPlugins;
use avian2d::math::Vector;
use avian2d::prelude::*;
use bevy::asset::io::{AssetSource, AssetSourceBuilder};
use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::render::render_resource::TextureFormat;
use bevy::{asset::AssetMetaCheck, prelude::*};
use vpin::vpx::units::vpu_to_m;
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
        // Headless render mode (VPINBALL_HEADLESS=1): run without a window, rendering the
        // main view to an offscreen image that the remote control `screenshot` command
        // can save. Lets the output be inspected where a window cannot be presented
        // (CI, sandboxes). Zero effect on normal runs.
        let headless = std::env::var("VPINBALL_HEADLESS").is_ok();
        app.insert_resource(Headless(headless));

        // Tables are read from a folder on the filesystem (default `~/vpinball/tables`)
        // rather than the app's `assets` folder. Register that folder as the read-only
        // `tables` asset source; this must happen before `AssetPlugin` (added by
        // `DefaultPlugins`). No file watcher: `.vpx` files are not hot-edited, and
        // recursively watching a large tables library can exhaust inotify watches.
        let (tables_dir, cli_table) = resolve_tables();
        app.register_asset_source(
            TABLES_SOURCE,
            AssetSourceBuilder::new(AssetSource::get_default_reader(
                tables_dir.to_string_lossy().into_owned(),
            )),
        );
        app.insert_resource(TablesDir(tables_dir));

        let default_plugins = DefaultPlugins
            .set(AssetPlugin {
                // Wasm builds will check for meta files (that don't exist) if this isn't set.
                // This causes errors and even panics on web build on itch.
                // See https://github.com/bevyengine/bevy_github_ci_template/issues/48.
                meta_check: AssetMetaCheck::Never,
                ..default()
            })
            .set(AudioPlugin {
                default_spatial_scale: SpatialScale::new_2d(AUDIO_SCALE),
                ..default()
            });
        if headless {
            app.add_plugins(
                default_plugins
                    .set(WindowPlugin {
                        primary_window: None,
                        exit_condition: bevy::window::ExitCondition::DontExit,
                        close_when_requested: false,
                        ..default()
                    })
                    .disable::<bevy::winit::WinitPlugin>(),
            );
            app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
                core::time::Duration::from_millis(16),
            ));
        } else {
            app.add_plugins(
                default_plugins.set(WindowPlugin {
                    primary_window: Window {
                        title: "VPinball2D".to_string(),
                        // The application id (Wayland) / WM_CLASS (X11). Without it the
                        // compositor has no app to associate the window with: COSMIC/GNOME
                        // show an empty name in the top bar and cannot restore the window
                        // after it is minimized (you have to alt-tab back to it).
                        name: Some("vpinball2d".to_string()),
                        fit_canvas_to_parent: true,
                        ..default()
                    }
                    .into(),
                    ..default()
                }),
            );
        }

        // One unit in bevy is one meter
        // Interpolate rigid-body Transforms between fixed physics steps so rendering stays
        // smooth when the step rate is low relative to the framerate (e.g. in slow motion).
        app.add_plugins(
            PhysicsPlugins::default()
                .with_length_unit(0.1)
                // The single app collision hook (avian allows one): one-way gates yield in their
                // open direction. See `pinball::gate`.
                .with_collision_hooks::<crate::pinball::gate::GateCollisionHooks>()
                .set(PhysicsInterpolationPlugin::interpolate_all()),
        );
        // Default until a table loads; spawn_level sets the table's own gravity
        // (vpinball's sin(slope) * gravity, see pinball::level).
        app.insert_resource(Gravity(Vector::NEG_Y * 9.81 * 0.12192));
        // to improve physics stability
        app.insert_resource(SubstepCount(50));

        // #[cfg(feature = "dev")]
        // app.add_plugins((EguiPlugin::default(), WorldInspectorPlugin::new()));

        // Add other plugins.
        app.add_plugins((
            VpxPlugin,
            tables::plugin,
            asset_tracking::plugin,
            audio::plugin,
            pinball::plugin,
            scripting::plugin,
            flexdmd::render::plugin,
            #[cfg(feature = "dev")]
            dev_tools::plugin,
            #[cfg(any(feature = "remote_control", feature = "telemetry"))]
            play::plugin,
            screens::plugin,
            theme::plugin,
        ));

        // A table given on the command line (e.g. `vpinball2d "My Table.vpx"`) is an
        // external frontend driving us: skip the picker, load it straight away, and
        // make Esc exit the game instead of returning to selection.
        if let Some(table) = cli_table {
            app.insert_resource(crate::pinball::TablePath::new(table));
            app.insert_resource(crate::screens::ExternalFrontend(true));
            app.insert_state(crate::screens::Screen::Loading);
        }

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
        // Physics-schedule collision-response systems (e.g. slingshots) also honour pause.
        app.configure_sets(
            FixedPostUpdate,
            PausableSystems.run_if(in_state(Pause(false))),
        );

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

/// Whether the game is paused.
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
fn spawn_camera(
    mut commands: Commands,
    headless: Res<Headless>,
    mut images: ResMut<Assets<Image>>,
) {
    // The vpinball demo table is 2162 vpu units deep and 952 vpu units wide.
    let table_width_m = vpu_to_m(952.0);
    let table_depth_m = vpu_to_m(2162.0);
    // HDR + Bloom so the additive light glows (`pinball::light::GlowMaterial`,
    // blended `src=One, dst=One`) accumulate past 1.0 in a float buffer and bloom
    // into soft halos instead of hard-clipping to white on the LDR pipeline.
    // Camera2d keeps its default `Tonemapping::None`, so only the over-bright glows
    // bloom; the rest of the vpinball-matched playfield (all <= 1.0) is untouched.
    let camera = commands
        .spawn((
            Name::new("Camera"),
            Camera2d,
            bevy::camera::Hdr,
            // Subtle bloom: just a soft halo on the over-bright lights. Higher
            // intensities make NATURAL's energy-conserving spread wash the whole
            // image ("dirty window"); brightness comes from the lights themselves
            // (HDR headroom, see pinball::light) rather than from heavy bloom.
            bevy::post_process::bloom::Bloom {
                intensity: 0.08,
                ..bevy::post_process::bloom::Bloom::NATURAL
            },
            // Explicit UI camera so the menus render to this camera (and its target)
            // even when there is no primary window (headless capture).
            IsDefaultUiCamera,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: bevy::camera::ScalingMode::AutoMin {
                    min_height: table_depth_m,
                    min_width: table_width_m,
                },
                ..OrthographicProjection::default_2d()
            }),
        ))
        .id();
    if headless.0 {
        // No window to present to, so render the main view to an offscreen image that
        // the `screenshot` command captures.
        // Landscape, matching a desktop backdrop (16:9); the playfield renders in
        // the backdrop's central cutout (see pinball::desktop).
        let image = images.add(Image::new_target_texture(
            1920,
            1080,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        commands
            .entity(camera)
            .insert(bevy::camera::RenderTarget::Image(image.clone().into()));
        commands.insert_resource(HeadlessImage(image));
    }
}

/// Whether the app is running in headless render mode (no window).
#[derive(Resource)]
pub(crate) struct Headless(pub(crate) bool);

/// In headless mode, the offscreen image the main camera renders to; the remote
/// control `screenshot` command saves this instead of the (absent) window.
#[derive(Resource)]
pub(crate) struct HeadlessImage(pub(crate) Handle<Image>);
