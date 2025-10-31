//! Spawn the main level.

use crate::pinball::ball::ball;
use crate::pinball::bumper::spawn_bumper;
use crate::pinball::kicker::spawn_kicker;
use crate::pinball::light::spawn_light;
use crate::pinball::plunger::spawn_plunger;
use crate::pinball::rubber::spawn_rubber;
use crate::pinball::trigger::spawn_trigger;
use crate::pinball::wall::spawn_wall;
use crate::vpx::VpxAsset;
use crate::{
    pinball::table::{TableAssets, table},
    screens::Screen,
};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use vpin::vpx::gameitem::GameItemEnum;
use vpin::vpx::vpu_to_m;

pub(super) fn plugin(_app: &mut App) {
    //app.load_resource::<LevelAssets>();
}

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct LevelAssets {
    #[dependency]
    music: Handle<AudioSource>,
}

impl FromWorld for LevelAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            music: assets.load("audio/music/Fluffing A Duck.ogg"),
        }
    }
}

/// A system that spawns the main level.
pub fn spawn_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera_q: Query<(&Camera, &Projection), With<Camera2d>>,
) {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);
    let vpx_to_bevy_transform = Transform::from_xyz(-table_width_m / 2.0, table_depth_m / 2.0, 0.0);

    // TODO the walls should probably be children of the table
    commands
        .spawn((
            Name::new("Level"),
            Transform::default(),
            Visibility::default(),
            DespawnOnExit(Screen::Gameplay),
            children![table(
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
                window,
                camera_q,
            )],
        ))
        .with_children(|parent| {
            parent.spawn(ball(
                0,
                &table_assets,
                &mut meshes,
                &mut materials,
                &assets_vpx,
                Vec2::default(),
            ));
            // parent.spawn(ball(
            //     4,
            //     &table_assets,
            //     &mut meshes,
            //     &mut materials,
            //     &assets_vpx,
            // ));
        })
        .with_children(|parent| {
            vpx_asset.raw.gameitems.iter().for_each(|item| match item {
                GameItemEnum::Wall(wall) => {
                    // TODO on the example table wall 15 is a wall that keeps the
                    //   ball in in the lane and allows the plunger to pass through
                    //   However we don't know how to allow that behavior yet so we skip it for now
                    //   https://github.com/avianphysics/avian/blob/main/crates/avian2d/examples/one_way_platform_2d.rs
                    //   Maybe they should be on different collision layers?
                    if wall.name != "Wall15" {
                        spawn_wall(
                            parent,
                            &meshes,
                            &mut materials,
                            vpx_asset,
                            vpx_to_bevy_transform,
                            wall,
                        )
                    }
                }
                GameItemEnum::Bumper(bumper) => {
                    spawn_bumper(
                        parent,
                        &mut meshes,
                        &mut materials,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        bumper,
                    );
                }
                GameItemEnum::Trigger(trigger) => {
                    spawn_trigger(
                        &mut meshes,
                        &mut materials,
                        vpx_to_bevy_transform,
                        parent,
                        trigger,
                    );
                }
                GameItemEnum::Kicker(kicker) => {
                    // TODO implement kicker spawning
                    spawn_kicker(
                        &mut meshes,
                        &mut materials,
                        vpx_to_bevy_transform,
                        parent,
                        kicker,
                    );
                }
                GameItemEnum::Light(light) => {
                    spawn_light(
                        &mut meshes,
                        &mut materials,
                        vpx_to_bevy_transform,
                        parent,
                        light,
                    );
                }
                GameItemEnum::Rubber(rubber) => spawn_rubber(
                    &mut meshes,
                    &mut materials,
                    vpx_to_bevy_transform,
                    parent,
                    rubber,
                    vpx_asset,
                ),
                GameItemEnum::Plunger(plunger) => spawn_plunger(
                    &mut meshes,
                    &mut materials,
                    vpx_to_bevy_transform,
                    parent,
                    plunger,
                    vpx_asset,
                ),
                _ => (),
            });
        });
}
