//! The gameplay screen: spawn the selected table and play it.

use crate::pinball::level::spawn_level;
use crate::pinball::table::TableAssets;
use crate::screens::{ExternalFrontend, Screen};
use crate::vpx::VpxAsset;
use bevy::{input::common_conditions::input_just_pressed, prelude::*};
use vpin::vpx::units::vpu_to_m;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(OnEnter(Screen::Gameplay), (spawn_level, fit_camera));
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
    mut cameras: Query<
        &mut Projection,
        (
            With<Camera2d>,
            Without<crate::pinball::lightmap::LightmapCamera>,
        ),
    >,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);
    for mut projection in &mut cameras {
        if let Projection::Orthographic(ortho) = &mut *projection {
            ortho.scaling_mode = bevy::camera::ScalingMode::AutoMin {
                min_height: table_depth_m,
                min_width: table_width_m,
            };
        }
    }
}
