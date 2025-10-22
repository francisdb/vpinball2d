use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use crate::{AppSystems, PausableSystems};
use avian2d::prelude::*;
use bevy::audio::Volume;
use bevy::prelude::*;

// A typical pinball ball is
// 1-1/16 inches (27 mm) in diameter
const BALL_RADIUS_M: f32 = 0.027;

// A typical pinball ball mass is around 80 grams
const BALL_MASS_KG: f32 = 0.08;

#[derive(Component, Debug)]
pub struct Ball {
    id: u32,
}

pub(super) fn plugin(app: &mut App) {
    // Mouse ball control for development purposes
    app.add_systems(
        Update,
        (ball_roll, ball_collision_sounds)
            .in_set(AppSystems::RecordInput)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(crate) fn ball(
    id: u32,
    table_assets: &TableAssets,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    assets_vpx: &Res<Assets<VpxAsset>>,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    let ball_image = vpx_asset
        .named_images
        .get(vpx_asset.raw.gamedata.ball_image.as_str())
        .unwrap();
    let ball_material = materials.add(ColorMaterial {
        texture: Some(ball_image.clone()),
        ..default()
    });
    // TODO add ball collision sound effects
    // We'll have to be a bit more creative here since ball sounds are actually handled by the script in vpinball.
    let sound_roll = vpx_asset.named_sounds.get("fx_ballrolling0").unwrap();
    (
        Name::from(format!("Ball {}", id)),
        Ball { id },
        Mesh2d::from(meshes.add(Mesh::from(Circle::new(BALL_RADIUS_M)))),
        MeshMaterial2d::from(ball_material),
        // physics components
        RigidBody::Dynamic,
        Mass::from(BALL_MASS_KG),
        Restitution::new(0.4),
        Friction::from(0.2),
        Collider::circle(BALL_RADIUS_M),
        SleepingDisabled,
        CollisionEventsEnabled,
        // sound component
        AudioPlayer::new(sound_roll.clone()),
        PlaybackSettings::LOOP.with_spatial(true),
    )
}

fn ball_roll(mut ball_query: Query<(&LinearVelocity, &mut SpatialAudioSink), With<Ball>>) {
    // for non-spatial audio, use AudioSink instead of SpatialAudioSink
    const MINIMAL_VELOCITY: f32 = 0.005;
    for (velocity, mut sink) in ball_query.iter_mut() {
        let speed = velocity.0.length();
        //println!("Speed: {}", speed);
        if velocity.0.length() > MINIMAL_VELOCITY {
            sink.play();
            let volume = vol(speed);
            //println!("Volume: {}", volume);
            sink.set_volume(Volume::Linear(volume));
            // TODO setting pitch seems to mess with the panning of the spatial audio
            //   not sure if this is a bevy bug or something else
            //let pitch = pitch(speed);
            //println!("Pitch: {}", pitch);
            //sink.set_speed(pitch);
        } else {
            sink.pause();
        }
    }
}

/// Calculates the Volume of the sound based on the ball speed
fn vol(ball_speed: f32) -> f32 {
    (ball_speed * 5.0).clamp(0.0, 40.0)
}

fn collision_vol(collision_speed: f32) -> f32 {
    (collision_speed * 4.0).clamp(0.0, 10.0)
}

// /// Calculates the pitch of the sound based on the ball speed
// fn pitch(ball_speed: f32) -> f32 {
//     (ball_speed * 0.6).clamp(0.5, 1.5)
// }

/// when 2 balls collide, play a sound based on their combined speed
fn ball_collision_sounds(
    mut collision_reader: MessageReader<CollisionStart>,
    ball_query: Query<(&Ball, &LinearVelocity, &Transform), With<Ball>>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for event in collision_reader.read() {
        if let (Some(entity1), Some(entity2)) = (event.body1, event.body2)
            && ball_query.contains(entity1)
            && ball_query.contains(entity2)
        {
            // TODO the case where 2 balls simultaneously collide with each other and another object (like a vertical drop)
            //   gives us no sound which is incorrect
            let (ball1, vel1, transform1) = ball_query.get(entity1).unwrap();
            let (ball2, vel2, transform2) = ball_query.get(entity2).unwrap();
            debug!("Ball collision event between {:?} and {:?}", ball1, ball2);
            let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
            let sound_ball_collision = vpx_asset.named_sounds.get("fx_collide").unwrap();

            let distance_vec = vel1.0 - vel2.0;
            let combined_speed = distance_vec.length();
            let volume = collision_vol(combined_speed);
            let center_pos = (transform1.translation + transform2.translation) / 2.0;
            commands.spawn((
                AudioPlayer::new(sound_ball_collision.clone()),
                PlaybackSettings::ONCE
                    .with_spatial(true)
                    .with_volume(Volume::Linear(volume)),
                Transform::from_translation(center_pos),
            ));
        }
    }
}
