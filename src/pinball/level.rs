//! Spawn the main level.

use crate::pinball::ball::{BallAssets, BallMaterial, ball};
use crate::pinball::bumper::spawn_bumper;
use crate::pinball::flipper::spawn_flipper;
use crate::pinball::gate::spawn_gate;
use crate::pinball::kicker::spawn_kicker;
use crate::pinball::light::{GlowMaterial, InsertGlowMaterial, LightingAssets, spawn_light};
use crate::pinball::lightmap::{PlayfieldLightMaterial, lightmap_camera, lightmap_image};
use crate::pinball::plunger::spawn_plunger;
use crate::pinball::primitive::spawn_primitive;
use crate::pinball::ramp::spawn_ramp;
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
    mut ball_materials: ResMut<Assets<BallMaterial>>,
    mut glow_materials: ResMut<Assets<GlowMaterial>>,
    mut insert_glow_materials: ResMut<Assets<InsertGlowMaterial>>,
    mut playfield_materials: ResMut<Assets<PlayfieldLightMaterial>>,
    mut plastic_materials: ResMut<Assets<crate::pinball::lightmap::PlasticMaterial>>,
    mut atlas_layouts: ResMut<Assets<bevy::image::TextureAtlasLayout>>,
    mut images: ResMut<Assets<Image>>,
    ball_assets: Res<BallAssets>,
    lighting: Res<LightingAssets>,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
    camera_q: Query<(&Camera, &Projection), With<Camera2d>>,
    script: Option<Res<crate::scripting::ScriptActive>>,
) {
    // A table script owns the ball lifecycle and the lamp states; without one
    // the engine free-plays (auto ball, attract blinker).
    let script_active = script.is_some();
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let table_width_m = vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left);
    let table_depth_m = vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top);
    let vpx_to_bevy_transform = Transform::from_xyz(-table_width_m / 2.0, table_depth_m / 2.0, 0.0);

    // The overhead lamps all shadows are cast from: vpinball's scene lights, at the
    // height this table defines.
    let overhead_lights = crate::pinball::light::OverheadLights::for_table(
        table_depth_m,
        vpx_asset.raw.gamedata.light_height,
    );

    // Offscreen light/shadow map, rendered by its own camera over the playfield rect
    // and composited onto the playfield by `PlayfieldLightMaterial`.
    let light_map = lightmap_image(&mut images, table_width_m, table_depth_m);
    commands.spawn((
        lightmap_camera(light_map.clone(), table_width_m, table_depth_m),
        DespawnOnExit(Screen::Gameplay),
    ));

    // Static-shadow pass: every static shadow-casting item also renders into this
    // image on a transparent background; the light map darkens the playfield with it
    // projected away from each lamp (see `light::static_shadow_quads`).
    let static_render = lightmap_image(&mut images, table_width_m, table_depth_m);
    commands.spawn((
        crate::pinball::lightmap::static_shadow_camera(
            static_render.clone(),
            table_width_m,
            table_depth_m,
        ),
        DespawnOnExit(Screen::Gameplay),
    ));
    for quad in crate::pinball::light::static_shadow_quads(
        &mut meshes,
        &mut materials,
        &overhead_lights,
        static_render,
        table_width_m,
        table_depth_m,
    ) {
        commands.spawn(quad);
    }
    commands.insert_resource(overhead_lights);

    // The table's own gravity, vpinball's model (player.cpp / PhysicsEngine):
    // slope = lerp(tilt min, tilt max, global difficulty), in-plane acceleration
    // = sin(slope) * gravity. The vpx gravity is in VP units per 10 ms tick
    // squared; the default GRAVITYCONST 1.81751 is exactly 9.81 m/s^2.
    let gamedata = &vpx_asset.raw.gamedata;
    let slope_deg = gamedata.angle_tilt_min
        + (gamedata.angle_tilt_max - gamedata.angle_tilt_min)
            * gamedata.global_difficulty.clamp(0.0, 1.0);
    // VPU/tick^2 -> m/s^2 (tick = 10 ms).
    let gravity_m_s2 = vpu_to_m(gamedata.gravity) / (0.01 * 0.01);
    commands.insert_resource(avian2d::prelude::Gravity(
        avian2d::math::Vector::NEG_Y * gravity_m_s2 * slope_deg.to_radians().sin(),
    ));
    info!(
        "Table gravity: slope {slope_deg:.1} deg, g {gravity_m_s2:.2} m/s^2 -> {:.3} m/s^2 down-table",
        gravity_m_s2 * slope_deg.to_radians().sin()
    );

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
            if !script_active {
                parent.spawn(ball(
                    0,
                    &table_assets,
                    &mut meshes,
                    &mut ball_materials,
                    &ball_assets,
                    &assets_vpx,
                    Vec2::default(),
                ));
            }
            // parent.spawn(ball(
            //     4,
            //     &table_assets,
            //     &mut meshes,
            //     &mut materials,
            //     &assets_vpx,
            // ));
        })
        .with_children(|parent| {
            // The index in the gameitem list breaks render-layer ties (see layer.rs).
            vpx_asset
                .raw
                .gameitems
                .iter()
                .enumerate()
                .for_each(|(item_index, item)| match item {
                    GameItemEnum::Wall(wall) => spawn_wall(
                        parent,
                        &mut meshes,
                        &mut materials,
                        &mut plastic_materials,
                        &light_map,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        wall,
                        item_index,
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
                            &mut insert_glow_materials,
                            &lighting.glow,
                            vpx_asset,
                            Vec2::new(table_width_m, table_depth_m),
                            vpx_to_bevy_transform,
                            parent,
                            light,
                            script_active,
                        );
                    }
                    GameItemEnum::Rubber(rubber) => spawn_rubber(
                        &mut meshes,
                        &mut materials,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        parent,
                        rubber,
                        item_index,
                    ),
                    GameItemEnum::Plunger(plunger) => spawn_plunger(
                        &mut meshes,
                        &mut materials,
                        &mut images,
                        vpx_asset,
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
                        vpx_asset,
                    ),
                    GameItemEnum::HitTarget(target) => spawn_target(
                        parent,
                        &mut meshes,
                        &mut materials,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        target,
                        item_index,
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
                    GameItemEnum::Ramp(ramp) => spawn_ramp(
                        parent,
                        &meshes,
                        &mut materials,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        ramp,
                        item_index,
                    ),
                    GameItemEnum::Primitive(primitive) => spawn_primitive(
                        parent,
                        &mut materials,
                        vpx_asset,
                        vpx_to_bevy_transform,
                        primitive,
                        item_index,
                    ),
                    GameItemEnum::Flasher(flasher) => {
                        crate::pinball::flasher::spawn_flasher(
                            parent,
                            &mut meshes,
                            &mut materials,
                            vpx_asset,
                            vpx_to_bevy_transform,
                            flasher,
                            item_index,
                        );
                    }
                    GameItemEnum::Reel(reel) => {
                        crate::pinball::reel::spawn_reel(
                            parent,
                            &mut atlas_layouts,
                            vpx_asset,
                            vpx_to_bevy_transform,
                            reel,
                        );
                    }
                    _ => (),
                });
        });
}
