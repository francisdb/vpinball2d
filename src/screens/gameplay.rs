//! The gameplay screen: spawn the selected table and play it.

use crate::pinball::level::spawn_level;
use crate::screens::{ExternalFrontend, Screen};
use bevy::{input::common_conditions::input_just_pressed, prelude::*};

pub(super) fn plugin(app: &mut App) {
    // The table script (if any) loads first so the level spawn can adapt
    // (no auto ball, lights start in their authored state).
    app.add_systems(
        OnEnter(Screen::Gameplay),
        (crate::scripting::init_script, spawn_level, fit_camera).chain(),
    );
    app.add_systems(
        Update,
        leave_gameplay.run_if(in_state(Screen::Gameplay).and(input_just_pressed(KeyCode::Escape))),
    );
}

/// Esc leaves the table: back to the picker normally, or exit the game when an
/// external frontend selected the table on the command line.
fn leave_gameplay(
    external: Res<ExternalFrontend>,
    mut next_screen: ResMut<NextState<Screen>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if external.0 {
        app_exit.write(AppExit::Success);
    } else {
        next_screen.set(Screen::TableSelect);
    }
}

fn fit_camera(
    mut commands: Commands,
    mut cameras: Query<
        (Entity, &mut Projection, &mut Transform, &Camera),
        (
            With<Camera2d>,
            Without<crate::pinball::lightmap::LightmapCamera>,
        ),
    >,
    layout: Res<crate::pinball::desktop::DesktopLayout>,
) {
    use bevy::camera::CameraProjection;
    // Show the whole desktop backdrop (the playfield sits in its cutout at the
    // origin); the reels are overlaid on the backdrop's printed windows. The
    // backdrop's cutout is not vertically centred in it, so centre the camera on
    // the backdrop rather than the origin (the nudge shake offsets from this).
    for (entity, mut projection, mut transform, camera) in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = bevy::camera::ScalingMode::AutoMin {
                min_height: layout.size.y,
                min_width: layout.size.x,
            };
            // The main camera renders to a fixed-size image target (headless) or a
            // window; Bevy only recomputes `area` when that target resizes, so a
            // mid-run `scaling_mode` change is otherwise ignored. Recompute it here
            // from the target's pixel size so the view matches the backdrop exactly
            // (the reels rely on this to land on their windows).
            if let Some(size) = camera.physical_target_size() {
                ortho.update(size.x as f32, size.y as f32);
            }
        }
        transform.translation.x = layout.center.x;
        transform.translation.y = layout.center.y;
        commands
            .entity(entity)
            .insert(crate::pinball::nudge::CameraRest(layout.center));
    }
}
