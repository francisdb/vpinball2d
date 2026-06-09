use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use crate::{AppSystems, PausableSystems, Pause};
use avian2d::prelude::*;
use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;

// A typical pinball ball is
// 1-1/16 inches (27 mm) in diameter
pub const BALL_RADIUS_M: f32 = 0.027 / 2.0;

// A typical pinball ball mass is around 80 grams
const BALL_MASS_KG: f32 = 0.08;

// Fallback colour for tables that ship no ball image: a light steel grey.
const STEEL_BALL_COLOR: Color = Color::srgb(0.8, 0.81, 0.84);

#[derive(Component, Debug)]
pub struct Ball {
    #[allow(unused)]
    pub(crate) id: u32,
}

pub(super) fn plugin(app: &mut App) {
    // Mouse ball control for development purposes
    app.add_systems(
        Update,
        (ball_roll, ball_collision_sounds) //
            .in_set(AppSystems::RecordInput)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
    app.add_systems(Update, mute_rolling.run_if(in_state(Pause(true))));
    // Attach the looping rolling sound when a ball spawns, if the table ships one.
    app.add_observer(attach_rolling_sound);
}

pub(crate) fn ball(
    id: u32,
    table_assets: &TableAssets,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    assets_vpx: &Res<Assets<VpxAsset>>,
    location: Vec2,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    // Best effort: not every table ships a ball image; fall back to a plain steel
    // colour so the ball still renders.
    let ball_material = match vpx_asset.image(vpx_asset.raw.gamedata.ball_image.as_str()) {
        Some(ball_image) => materials.add(ColorMaterial {
            texture: Some(ball_image.clone()),
            ..default()
        }),
        None => {
            warn!(
                "Ball image '{}' not found in table '{}'; using a plain steel ball",
                vpx_asset.raw.gamedata.ball_image, table_assets.file_name
            );
            materials.add(ColorMaterial::from(STEEL_BALL_COLOR))
        }
    };
    let ball_mesh = meshes.add(Mesh::from(Circle::new(BALL_RADIUS_M)));

    (
        Name::from(format!("Ball {id}")),
        Ball { id },
        Mesh2d::from(ball_mesh),
        MeshMaterial2d::from(ball_material),
        Transform::from_xyz(location.x, location.y, BALL_RADIUS_M),
        // physics components (grouped to stay within the bundle tuple arity limit)
        (
            RigidBody::Dynamic,
            Mass::from(BALL_MASS_KG),
            // vpinball applies the *hit object's* elasticity and friction alone in a ball/wall
            // collision (see HitBall::Collide3DWall); the ball contributes neither. We mirror that by
            // making the ball "transparent": coefficient 1.0 with a `Min` combine rule, so the result
            // is always the surface's own value (`min(1.0, surface) == surface`). Otherwise avian's
            // default `Average` mixes the ball's restitution into every surface, making metal rails
            // (elasticity ~0.3) feel like rubber (0.35+) so the ball bounces in lanes instead of
            // sliding.
            Restitution::new(1.0).with_combine_rule(CoefficientCombine::Min),
            Friction::new(1.0).with_combine_rule(CoefficientCombine::Min),
            Collider::circle(BALL_RADIUS_M),
            SleepingDisabled,
            CollisionEventsEnabled,
            // Run the app's collision hook on every ball contact: the one-way gate logic
            // (see pinball::gate::GateCollisionHooks).
            ActiveCollisionHooks::MODIFY_CONTACTS,
            // continuous collision detection to prevent tunneling at high speeds
            SweptCcd::default(),
        ),
        // The looping rolling sound is attached separately by `attach_rolling_sound`,
        // since not every table ships one (best effort: a missing sound is not fatal).
    )
}

/// On ball spawn, attach the looping rolling sound the table ships, if any.
///
/// Ball sounds are normally driven by the table script in vpinball; until that
/// is in place we just loop whatever "ball rolling" sound the table provides.
/// Tables name it inconsistently (`fx_ballrolling0`, `SY_TNA_REV02_Ball_Roll_0`,
/// ...), so we match on the name rather than a fixed key. A table without such a
/// sound simply rolls silently.
fn attach_rolling_sound(
    add: On<Add, Ball>,
    mut commands: Commands,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    let Some(table_assets) = table_assets else {
        return;
    };
    let Some(vpx_asset) = assets_vpx.get(&table_assets.vpx) else {
        return;
    };
    let Some(sound) = find_rolling_sound(vpx_asset) else {
        warn!(
            "No ball rolling sound found in table '{}'; the ball will roll silently",
            table_assets.file_name
        );
        return;
    };
    commands.entity(add.entity).insert((
        AudioPlayer::new(sound.clone()),
        PlaybackSettings::LOOP.with_spatial(true),
    ));
}

/// Best-effort lookup of a table's "ball rolling" sound by name. Sound names are
/// normalised (non-alphanumerics dropped, lowercased) and matched on containing
/// `ballroll`, which catches `fx_ballrolling0`, `SY_..._Ball_Roll_0`, etc.
fn find_rolling_sound(vpx_asset: &VpxAsset) -> Option<&Handle<AudioSource>> {
    vpx_asset
        .named_sounds
        .iter()
        .find(|(name, _)| {
            let normalized: String = name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect();
            normalized.contains("ballroll")
        })
        .map(|(_, handle)| handle)
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

fn mute_rolling(ball_query: Query<&mut SpatialAudioSink, With<Ball>>) {
    for sink in ball_query.iter() {
        sink.pause();
    }
}

/// Calculates the Volume of the sound based on the ball speed
fn vol(ball_speed: f32) -> f32 {
    (ball_speed * 5.0).clamp(0.0, 40.0)
}

fn collision_vol(collision_speed: f32) -> f32 {
    (collision_speed * 10.0).clamp(0.0, 10.0)
}

// /// Calculates the pitch of the sound based on the ball speed
// fn pitch(ball_speed: f32) -> f32 {
//     (ball_speed * 0.6).clamp(0.5, 1.5)
// }

/// when 2 balls collide, play a sound based on their combined speed
fn ball_collision_sounds(
    mut collision_reader: MessageReader<CollisionStart>,
    collisions: Collisions,
    ball_query: Query<&Transform, With<Ball>>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
) {
    for event in collision_reader.read() {
        if let (Some(entity1), Some(entity2)) = (event.body1, event.body2)
            && ball_query.contains(entity1)
            && ball_query.contains(entity2)
        {
            let transform1 = ball_query.get(entity1).unwrap();
            let transform2 = ball_query.get(entity2).unwrap();
            let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
            let sound_ball_collision = vpx_asset.named_sounds.get("fx_collide").unwrap();
            let Some(collision) = collisions.get(entity1, entity2) else {
                warn!(
                    "No collision info found for entities {:?} and {:?}",
                    entity1, entity2
                );
                continue;
            };
            let impulse = collision.total_normal_impulse_magnitude();
            let volume = collision_vol(impulse);
            let center_pos = (transform1.translation + transform2.translation) / 2.0;
            commands.spawn((
                AudioPlayer::new(sound_ball_collision.clone()),
                // DESPAWN (not ONCE) so the one-shot entity cleans itself up.
                PlaybackSettings::DESPAWN
                    .with_spatial(true)
                    .with_volume(Volume::Linear(volume)),
                Transform::from_translation(center_pos),
            ));
        }
    }
}
