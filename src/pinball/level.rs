//! Spawn the main level.

use crate::pinball::ball::ball;
use crate::pinball::bumper::spawn_bumper;
use crate::pinball::flipper::spawn_flipper;
use crate::pinball::gate::spawn_gate;
use crate::pinball::kicker::spawn_kicker;
use crate::pinball::light::{GlowMaterial, LightingAssets, spawn_light};
use crate::pinball::lightmap::{PlayfieldLightMaterial, lightmap_camera, lightmap_image};
use crate::pinball::plunger::spawn_plunger;
use crate::pinball::rubber::spawn_rubber;
use crate::pinball::spinner::spawn_spinner;
use crate::pinball::targets::spawn_target;
use crate::pinball::trigger::spawn_trigger;
use crate::pinball::wall::spawn_wall;
use crate::vpx::VpxAsset;
use crate::{
    pinball::table::{TableAssets, table},
    screens::Screen,
};
use bevy::prelude::*;
use vpin::vpx::gameitem::GameItemEnum;
use vpin::vpx::units::vpu_to_m;

pub(super) fn plugin(app: &mut App) {
    //app.load_resource::<LevelAssets>();
    app.add_plugins(crate::pinball::playfield::plugin);
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
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut playfield_materials: ResMut<Assets<PlayfieldLightMaterial>>,
    mut images: ResMut<Assets<Image>>,
    lighting: Res<LightingAssets>,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
    camera_q: Query<(&Camera, &Projection), With<Camera2d>>,
) {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);
    let vpx_to_bevy_transform = Transform::from_xyz(-table_width_m / 2.0, table_depth_m / 2.0, 0.0);

    // Offscreen light/shadow map, rendered by its own camera over the playfield rect
    // and composited onto the playfield by `PlayfieldLightMaterial`.
    let light_map = lightmap_image(&mut images, table_width_m, table_depth_m);
    commands.spawn((
        lightmap_camera(light_map.clone(), table_width_m, table_depth_m),
        DespawnOnExit(Screen::Gameplay),
    ));

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
                &mut playfield_materials,
                &mut images,
                light_map.clone(),
                &assets_vpx,
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
                GameItemEnum::Wall(wall) => spawn_wall(
                    parent,
                    &meshes,
                    &mut materials,
                    vpx_asset,
                    vpx_to_bevy_transform,
                    wall,
                ),
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
                        &mut glow_materials,
                        &lighting.glow,
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
                ),
                GameItemEnum::Plunger(plunger) => spawn_plunger(
                    &mut meshes,
                    &mut materials,
                    vpx_to_bevy_transform,
                    parent,
                    plunger,
                ),
                GameItemEnum::Flipper(flipper) => spawn_flipper(
                    &mut meshes,
                    &mut materials,
                    vpx_to_bevy_transform,
                    parent,
                    flipper,
                    vpx_asset.raw.gamedata.materials.as_deref().unwrap_or(&[]),
                ),
                GameItemEnum::HitTarget(target) => spawn_target(
                    parent,
                    &mut meshes,
                    &mut materials,
                    vpx_asset,
                    vpx_to_bevy_transform,
                    target,
                ),
                GameItemEnum::Spinner(spinner) => spawn_spinner(
                    parent,
                    &mut meshes,
                    &mut materials,
                    vpx_asset,
                    vpx_to_bevy_transform,
                    spinner,
                ),
                GameItemEnum::Gate(gate) => spawn_gate(
                    parent,
                    &mut meshes,
                    &mut materials,
                    vpx_asset,
                    vpx_to_bevy_transform,
                    gate,
                ),
                _ => (),
            });
        });
}
