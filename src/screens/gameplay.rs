//! The screen state for the main gameplay.

use crate::pinball::table::TableAssets;
use crate::vpx::VpxAsset;
use crate::{Pause, menus::Menu, pinball::level::spawn_level, screens::Screen};
use avian2d::prelude::*;
use bevy::{input::common_conditions::input_just_pressed, prelude::*};
use vpin::vpx::vpu_to_m;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(Screen::GameSetup),
        (spawn_level, enter_gameplay_screen).chain(),
    );
    app.add_systems(OnEnter(Screen::GameSetup), fit_camera);

    // we need to first spawn entities before we can run the script that starts on Screen::Gameplay

    // Toggle pause on key press.
    app.add_systems(
        Update,
        (
            (pause, spawn_pause_overlay, open_pause_menu).run_if(
                in_state(Screen::Gameplay)
                    .and(in_state(Menu::None))
                    .and(input_just_pressed(KeyCode::KeyP).or(input_just_pressed(KeyCode::Escape))),
            ),
            close_menu.run_if(
                in_state(Screen::Gameplay)
                    .and(not(in_state(Menu::None)))
                    .and(input_just_pressed(KeyCode::KeyP)),
            ),
        ),
    );
    app.add_systems(OnExit(Screen::Gameplay), (close_menu, unpause));
    app.add_systems(
        OnEnter(Menu::None),
        unpause.run_if(in_state(Screen::Gameplay)),
    );
}

fn enter_gameplay_screen(mut next_screen: ResMut<NextState<Screen>>) {
    next_screen.set(Screen::Gameplay);
}

fn unpause(mut next_pause: ResMut<NextState<Pause>>, mut time: ResMut<Time<Physics>>) {
    next_pause.set(Pause(false));
    time.unpause();
}

fn pause(mut next_pause: ResMut<NextState<Pause>>, mut time: ResMut<Time<Physics>>) {
    next_pause.set(Pause(true));
    time.pause();
}

fn spawn_pause_overlay(mut commands: Commands) {
    commands.spawn((
        Name::new("Pause Overlay"),
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
        GlobalZIndex(1),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        DespawnOnExit(Pause(true)),
    ));
}

fn open_pause_menu(mut next_menu: ResMut<NextState<Menu>>) {
    next_menu.set(Menu::Pause);
}

fn close_menu(mut next_menu: ResMut<NextState<Menu>>) {
    next_menu.set(Menu::None);
}

fn fit_camera(
    mut cameras: Query<&mut Projection, With<Camera2d>>,
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
