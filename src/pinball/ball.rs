use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use crate::{AppSystems, PausableSystems, Pause};
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::audio::{AudioSource, Volume};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use vpin::vpx::units::vpu_to_m;

// A typical pinball ball is
// 1-1/16 inches (27 mm) in diameter
pub const BALL_RADIUS_M: f32 = 0.027 / 2.0;

// A typical pinball ball mass is around 80 grams
const BALL_MASS_KG: f32 = 0.08;

// Overhead light directions (screen space, y up) the chrome ball reflects as
// specular hotspots. These roughly mirror the two overhead lamps the shadow system
// casts from (`light::OverheadLights`), as seen from the table centre.
const BALL_LIGHTS: [Vec2; 2] = [Vec2::new(0.5, 0.7), Vec2::new(-0.5, 0.7)];
// How high the overhead lights sit above the playfield plane, and how strong
// their specular hotspot on the ball is.
const BALL_LIGHT_ELEVATION: f32 = 0.8;
const BALL_LIGHT_INTENSITY: f32 = 0.7;
// Specular exponent: higher is a tighter, sharper highlight (polished chrome).
const BALL_SHININESS: f32 = 80.0;
// Subtle cool steel tint applied to the reflected environment.
const BALL_TINT: Vec3 = Vec3::new(0.95, 0.96, 1.0);
// How strongly the ball's rim reflects the nearby playfield art (0..1).
const PLAYFIELD_REFLECTION_STRENGTH: f32 = 0.7;
// How far around the ball (metres) the table reflection reaches.
const REFLECTION_SPREAD_M: f32 = 0.04;
// How strongly the surface decal (scratches/logo) shows on the ball (0..1).
const DECAL_STRENGTH: f32 = 0.4;

#[derive(Component, Debug)]
pub struct Ball {
    #[allow(unused)]
    pub(crate) id: u32,
}

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<BallMaterial>::default());
    app.add_systems(Startup, setup_ball_assets);
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

/// Shader inputs for [`BallMaterial`]; see `shaders/ball.wgsl`.
#[derive(Clone, Copy, ShaderType)]
struct BallUniform {
    /// rgb = reflection tint, a = specular shininess.
    tint: Vec4,
    /// xy = screen-space direction to the light (y up), z = elevation, w = intensity.
    light0: Vec4,
    light1: Vec4,
    /// xy = playfield size (m), z = table reflection strength, w = reflection spread (m).
    playfield: Vec4,
    /// x = decal strength, y = decal mode (0 = scratches/additive, 1 = logo/screen).
    decal: Vec4,
}

/// Renders the ball as polished chrome: the per-pixel sphere normal reflects a
/// distant environment map (`env`: the table's ball image when it ships one, an
/// environment map as in vpinball, otherwise the neutral studio gradient from
/// [`BallAssets`]) and the nearby `playfield` art, plus the two overhead lights.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct BallMaterial {
    #[uniform(0)]
    uniform: BallUniform,
    #[texture(1)]
    #[sampler(2)]
    env: Handle<Image>,
    #[texture(3)]
    #[sampler(4)]
    playfield: Handle<Image>,
    #[texture(5)]
    #[sampler(6)]
    decal: Handle<Image>,
}

impl BallMaterial {
    #[allow(clippy::too_many_arguments)]
    fn chrome(
        env: Handle<Image>,
        playfield: Handle<Image>,
        size: Vec2,
        pf_strength: f32,
        decal: Handle<Image>,
        decal_strength: f32,
        decal_mode: bool,
    ) -> Self {
        let light = |dir: Vec2| {
            dir.normalize()
                .extend(BALL_LIGHT_ELEVATION)
                .extend(BALL_LIGHT_INTENSITY)
        };
        Self {
            uniform: BallUniform {
                tint: BALL_TINT.extend(BALL_SHININESS),
                light0: light(BALL_LIGHTS[0]),
                light1: light(BALL_LIGHTS[1]),
                playfield: size.extend(pf_strength).extend(REFLECTION_SPREAD_M),
                decal: Vec4::new(decal_strength, if decal_mode { 1.0 } else { 0.0 }, 0.0, 0.0),
            },
            env,
            playfield,
            decal,
        }
    }
}

impl Material2d for BallMaterial {
    // Custom vertex stage too: it drops the ball's spin so the reflection stays
    // fixed in space (see shaders/ball.wgsl).
    fn vertex_shader() -> ShaderRef {
        "shaders/ball.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/ball.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

/// Shared ball assets, built once at startup.
#[derive(Resource)]
pub(crate) struct BallAssets {
    /// Fallback environment map for tables that ship no ball image.
    default_env: Handle<Image>,
}

fn setup_ball_assets(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(BallAssets {
        default_env: images.add(default_env_image()),
    });
}

/// A neutral "studio" environment used when a table ships no ball image: an
/// equirectangular vertical gradient, brighter overhead (ceiling/lights) and dark
/// below, so even a default ball reads as polished steel. Width is tiny since the
/// gradient only varies with latitude.
fn default_env_image() -> Image {
    const W: u32 = 4;
    const H: u32 = 64;
    let top = Vec3::new(0.62, 0.64, 0.70);
    let bottom = Vec3::new(0.04, 0.04, 0.06);
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        // t: 0 at the top row (overhead) -> 1 at the bottom.
        let t = y as f32 / (H - 1) as f32;
        // Bias the bright cap so most of the sphere reads a calm mid-tone.
        let c = bottom.lerp(top, (1.0 - t).powf(0.6));
        let rgba = [to_u8(c.x), to_u8(c.y), to_u8(c.z), 255];
        for _ in 0..W {
            data.extend_from_slice(&rgba);
        }
    }
    Image::new(
        Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

pub(crate) fn ball(
    id: u32,
    table_assets: &TableAssets,
    meshes: &mut ResMut<Assets<Mesh>>,
    ball_materials: &mut ResMut<Assets<BallMaterial>>,
    ball_assets: &BallAssets,
    assets_vpx: &Res<Assets<VpxAsset>>,
    location: Vec2,
) -> impl Bundle {
    let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
    // vpinball's ball image is an environment reflection map, not a flat texture;
    // use it as the distant chrome reflection when present, else the neutral default.
    let env = vpx_asset
        .image(vpx_asset.raw.gamedata.ball_image.as_str())
        .cloned()
        .unwrap_or_else(|| ball_assets.default_env.clone());
    // Nearby reflection: the playfield art the ball rolls over. If the table's
    // playfield image is missing, bind a placeholder and disable the reflection.
    let (playfield, pf_strength) = match vpx_asset.image(vpx_asset.raw.gamedata.image.as_str()) {
        Some(handle) => (handle.clone(), PLAYFIELD_REFLECTION_STRENGTH),
        None => (ball_assets.default_env.clone(), 0.0),
    };
    let size = Vec2::new(
        vpu_to_m(vpx_asset.raw.gamedata.right - vpx_asset.raw.gamedata.left),
        vpu_to_m(vpx_asset.raw.gamedata.bottom - vpx_asset.raw.gamedata.top),
    );
    // Surface decal: scratches/wear (additive) or a logo (screen), overlaid on the
    // chrome and spun with the ball. Disabled if the table ships none.
    let (decal, decal_strength) =
        match vpx_asset.image(vpx_asset.raw.gamedata.ball_image_front.as_str()) {
            Some(handle) => (handle.clone(), DECAL_STRENGTH),
            None => (ball_assets.default_env.clone(), 0.0),
        };
    let ball_material = ball_materials.add(BallMaterial::chrome(
        env,
        playfield,
        size,
        pf_strength,
        decal,
        decal_strength,
        vpx_asset.raw.gamedata.ball_decal_mode,
    ));
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
            // Best effort: not every table ships this collision sound.
            let Some(sound_ball_collision) = vpx_asset.named_sounds.get("fx_collide") else {
                continue;
            };
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
