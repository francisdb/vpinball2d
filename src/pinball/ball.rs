use crate::asset_tracking::LoadResource;
use crate::{AppSystems, PausableSystems};
use avian2d::prelude::*;
use bevy::audio::Volume;
use bevy::prelude::*;

use crate::screens::Screen;

// A typical pinball ball is
// 1-1/16 inches (27 mm) in diameter
const BALL_RADIUS_M: f32 = 0.027;

// A typical pinball ball mass is around 80 grams
const BALL_MASS_KG: f32 = 0.08;

#[derive(Component)]
pub struct Ball;

pub(super) fn plugin(app: &mut App) {
    app.load_resource::<BallAssets>();

    // Mouse ball control for development purposes
    app.add_systems(
        Update,
        ball_roll
            .in_set(AppSystems::RecordInput)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(crate) fn ball(
    ball_assets: &BallAssets,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
) -> impl Bundle {
    let ball_material = materials.add(ColorMaterial {
        texture: Some(ball_assets.image.clone()),
        ..default()
    });
    (
        Name::from("Ball"),
        Ball,
        Mesh2d::from(meshes.add(Mesh::from(Circle::new(BALL_RADIUS_M)))),
        MeshMaterial2d::from(ball_material),
        // physics components
        RigidBody::Dynamic,
        Mass::from(BALL_MASS_KG),
        Restitution::new(0.4),
        Friction::from(0.2),
        Collider::circle(BALL_RADIUS_M),
        SleepingDisabled,
        // sound component
        AudioPlayer::new(ball_assets.sound_roll.clone()),
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

// /// Calculates the pitch of the sound based on the ball speed
// fn pitch(ball_speed: f32) -> f32 {
//     (ball_speed * 0.6).clamp(0.5, 1.5)
// }

#[derive(Resource, Asset, Clone, Reflect)]
#[reflect(Resource)]
pub struct BallAssets {
    #[dependency]
    image: Handle<Image>,
    #[dependency]
    sound_roll: Handle<AudioSource>,
}

impl FromWorld for BallAssets {
    fn from_world(world: &mut World) -> Self {
        let assets = world.resource::<AssetServer>();
        Self {
            image: assets.load("images/JPBall-Dark2.png"),
            sound_roll: assets.load("audio/sound_effects/fx_ballrolling0.wav"),
            // TODO add ball collision sound effects
        }
    }
}
