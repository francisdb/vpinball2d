use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::lightmap::lightmap_layer;
use crate::screens::Screen;
use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::color::Srgba;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::image::Image;
use bevy::mesh::{Mesh, Mesh2d, MeshVertexBufferLayoutRef};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
    RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

/// Shared lighting assets, built once at startup and reused by every light and
/// ball shadow on the table.
#[derive(Resource)]
pub(crate) struct LightingAssets {
    /// Soft radial gradient: white, brightest at the center, fading to fully
    /// transparent at the edge. Tinted bright per light.
    pub(crate) glow: Handle<Image>,
}

/// Z of the light glows: a flat surface effect just above the playfield, below the
/// shadows and the ball. The vpx bulb height is irrelevant for 2D layering.
const LIGHT_Z: f32 = 0.005;
/// Additive glow alpha per unit of vpx light intensity. Pinball general
/// illumination uses many low-power bulbs (intensity ~4); inserts are far
/// brighter (~90). Scaling by intensity keeps GI gentle while lit inserts stand
/// out. The light map multiplies the playfield, so over-bright clips to full
/// playfield brightness (not white), letting us push this without washout.
const INTENSITY_TO_ALPHA: f32 = 0.1;
/// Cap so no single lamp dominates.
const MAX_GLOW_ALPHA: f32 = 0.9;

// --- Shadows: tune every shadow here, in one place ---
// Ball and static-object shadows both render plain dark shapes into the light map
// and are softened *uniformly* by the map's resolution, so they always match. The
// two values below are the only knobs for how every shadow reads.
/// How dark every shadow is (0 = none, 1 = black).
const SHADOW_ALPHA: f32 = 0.22;
/// Softness of every shadow: the light map is rendered at this height (px) and
/// upscaled onto the playfield, so a lower value blurs all shadows (and glows)
/// more. `lightmap` reads this so it stays the single shadow-softness knob.
pub(crate) const SHADOW_SOFTNESS_PX: u32 = 400;
/// Z of the shadows in the light map: above the glows, below the ball.
const SHADOW_Z: f32 = 0.012;
/// Direction each of the two overhead lights throws a shadow (away from the light).
const SHADOW_DIRS: [Vec2; 2] = [Vec2::new(-0.5, -0.7), Vec2::new(0.5, -0.7)];
/// Ball shadow blob radius relative to the ball, and its offset per overhead light.
const SHADOW_RADIUS: f32 = BALL_RADIUS_M * 1.6;
const BALL_SHADOW_OFFSET: f32 = BALL_RADIUS_M;
/// Static-object shadow offset per overhead light (metres).
const MESH_SHADOW_OFFSET: f32 = 0.014;

#[derive(Component)]
pub struct Light {
    #[allow(dead_code)]
    pub name: String,
}

/// Tracks a ball so its drop shadow follows it. The shadow is a separate entity
/// (not a child of the ball) so it does not inherit the ball's spin.
#[derive(Component)]
struct BallShadow {
    ball: Entity,
}

/// Marks a static object that drops a shadow into the light map. A dark copy of the
/// object's mesh is rendered into the map, offset per overhead light, so the shadow
/// matches the object's shape. `scale` enlarges the shadow about the object centre
/// (e.g. so a bumper's shadow clears its wider cap); use 1.0 for a 1:1 copy. Spawn
/// the object as a direct child of the level (so its `Transform` is world space) for
/// the shadow to land in the right place.
#[derive(Component)]
pub(crate) struct ShadowCaster {
    pub(crate) scale: f32,
}

/// Additive material for light glows: light is added to the playfield rather than
/// blended over it, so colours are brightened instead of washed out. Overlapping
/// lights accumulate. The additive blend state is set in [`Material2d::specialize`].
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct GlowMaterial {
    #[uniform(0)]
    color: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    texture: Handle<Image>,
}

impl Material2d for GlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/glow.wgsl".into()
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Additive: framebuffer += source. The shader premultiplies by the glow
        // falloff so transparent edges contribute nothing.
        const ADD_ONE: BlendComponent = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        };
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|fragment| fragment.targets.get_mut(0))
            .and_then(|target| target.as_mut())
        {
            target.blend = Some(BlendState {
                color: ADD_ONE,
                alpha: ADD_ONE,
            });
        }
        Ok(())
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<GlowMaterial>::default());
    app.add_systems(Startup, setup_lighting);
    app.add_systems(
        Update,
        (
            (spawn_ball_shadows, update_ball_shadows).chain(),
            (spawn_flipper_shadows, update_flipper_shadows).chain(),
        )
            .run_if(in_state(Screen::Gameplay)),
    );
    app.add_systems(
        Update,
        spawn_static_shadows.run_if(in_state(Screen::Gameplay)),
    );
}

fn setup_lighting(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Soft glow: square the falloff for a gentle, diffuse light.
    let glow = images.add(radial_image(|distance| {
        (1.0 - distance).clamp(0.0, 1.0).powf(2.0)
    }));
    commands.insert_resource(LightingAssets { glow });
}

/// The flat-dark material every shadow uses; its soft look comes entirely from the
/// low-resolution light map, so ball and static shadows match.
fn shadow_material(materials: &mut Assets<ColorMaterial>) -> Handle<ColorMaterial> {
    materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    })
}

/// Builds a white radial texture whose alpha is `falloff(distance)`, where
/// `distance` is 0 at the center and 1 at the inscribed-circle edge. The shape of
/// the falloff controls how soft or hard the glow/shadow reads.
fn radial_image(falloff: impl Fn(f32) -> f32) -> Image {
    const SIZE: u32 = 128;
    let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = SIZE as f32 / 2.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = (x as f32 + 0.5 - center) / center;
            let dy = (y as f32 + 0.5 - center) / center;
            let distance = (dx * dx + dy * dy).sqrt();
            let alpha = falloff(distance).clamp(0.0, 1.0);
            let i = ((y * SIZE + x) * 4) as usize;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (alpha * 255.0) as u8;
        }
    }
    Image::new(
        Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
}

pub(super) fn spawn_light(
    meshes: &mut ResMut<Assets<Mesh>>,
    glow_materials: &mut ResMut<Assets<GlowMaterial>>,
    glow_texture: &Handle<Image>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    light: &vpx::gameitem::light::Light,
) {
    // Tables ship with their lamps off; the ROM lights them during a game (and
    // animates them in attract mode). We do not drive a ROM, so render only general
    // illumination (GI / pfGI bulbs, on whenever the table is powered) plus any lamp
    // the table ships explicitly lit. State 0 = off, 1 = on, 2 = blinking; None =
    // unspecified, treated as off so insert-heavy tables do not light every lamp.
    let is_gi = light.name.to_lowercase().contains("gi");
    let is_on = light.state.is_some_and(|state| state != 0.0);
    if !is_gi && !is_on {
        return;
    }
    // Cover the falloff radius, but never collapse to nothing for lights that
    // only define a small core.
    let radius = vpu_to_m(light.falloff_radius).max(vpu_to_m(light.mesh_radius));
    // Glow tinted by the light colour; brightness scales with the lamp intensity
    // (capped), so many low-power GI bulbs stay gentle.
    let alpha = (light.intensity * INTENSITY_TO_ALPHA).clamp(0.0, MAX_GLOW_ALPHA);
    let glow_color = Srgba::rgb_u8(light.color.r, light.color.g, light.color.b).with_alpha(alpha);
    parent.spawn((
        Light {
            name: light.name.clone(),
        },
        Name::from(format!("Light {}", light.name)),
        Transform::from_xyz(
            vpx_to_bevy_transform.translation.x + vpu_to_m(light.center.x),
            vpx_to_bevy_transform.translation.y - vpu_to_m(light.center.y),
            LIGHT_Z,
        ),
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(glow_materials.add(GlowMaterial {
            color: Color::from(glow_color).to_linear(),
            texture: glow_texture.clone(),
        })),
        // Render into the light map only, not directly on screen.
        lightmap_layer(),
    ));
}

/// Tracks a flipper so its drop shadow follows the bat. Unlike the ball (whose
/// shadows hang off one non-rotating tracker), each flipper shadow is its own
/// entity: the shadow copies the flipper's position *and* rotation while its light
/// offset stays in world space; as a child the offset would swing with the bat,
/// as if the light moved.
#[derive(Component)]
struct FlipperShadow {
    flipper: Entity,
    /// World-space offset away from one of the overhead lights (metres).
    offset: Vec2,
}

/// Spawns the drop shadows of each flipper: a dark copy of its outline per overhead
/// light, softened by the low-res light map exactly like every other shadow.
fn spawn_flipper_shadows(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
    flippers: Query<(Entity, &Mesh2d), Added<crate::pinball::flipper::Flipper>>,
) {
    if flippers.is_empty() {
        return;
    }
    let material = shadow_material(&mut materials);
    for (flipper, mesh) in &flippers {
        for dir in SHADOW_DIRS {
            commands.spawn((
                Name::from("Flipper shadow"),
                FlipperShadow {
                    flipper,
                    offset: dir * MESH_SHADOW_OFFSET,
                },
                Mesh2d(mesh.0.clone()),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0.0, 0.0, SHADOW_Z),
                lightmap_layer(),
                DespawnOnExit(Screen::Gameplay),
            ));
        }
    }
}

/// Keeps each flipper shadow under its flipper: the exact bat pose at this instant
/// (position and rotation), pushed away from each light in world space.
fn update_flipper_shadows(
    flippers: Query<&Transform, Without<FlipperShadow>>,
    mut shadows: Query<(&FlipperShadow, &mut Transform)>,
) {
    for (shadow, mut transform) in &mut shadows {
        let Ok(flipper_transform) = flippers.get(shadow.flipper) else {
            continue;
        };
        transform.translation =
            (flipper_transform.translation.truncate() + shadow.offset).extend(SHADOW_Z);
        transform.rotation = flipper_transform.rotation;
    }
}

/// Spawns one drop shadow per ball: a plain dark disc per overhead light, softened
/// by the low-res light map exactly like the static shadows.
fn spawn_ball_shadows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    balls: Query<(Entity, &Transform), Added<Ball>>,
) {
    for (ball, ball_transform) in balls.iter() {
        let shadow_mesh = meshes.add(Circle::new(SHADOW_RADIUS));
        let material = shadow_material(&mut materials);
        let position = ball_transform.translation;
        commands.spawn((
            BallShadow { ball },
            Name::from("Ball shadow"),
            Transform::from_xyz(position.x, position.y, SHADOW_Z),
            Visibility::default(),
            DespawnOnExit(Screen::Gameplay),
            children![
                (
                    Mesh2d(shadow_mesh.clone()),
                    MeshMaterial2d(material.clone()),
                    Transform::from_xyz(
                        SHADOW_DIRS[0].x * BALL_SHADOW_OFFSET,
                        SHADOW_DIRS[0].y * BALL_SHADOW_OFFSET,
                        0.0,
                    ),
                    lightmap_layer(),
                ),
                (
                    Mesh2d(shadow_mesh),
                    MeshMaterial2d(material),
                    Transform::from_xyz(
                        SHADOW_DIRS[1].x * BALL_SHADOW_OFFSET,
                        SHADOW_DIRS[1].y * BALL_SHADOW_OFFSET,
                        0.0,
                    ),
                    lightmap_layer(),
                ),
            ],
        ));
    }
}

/// Keeps each ball shadow under its ball. The shadow only follows the ball's
/// position, not its rotation, so the offsets stay aligned to the lights.
fn update_ball_shadows(
    mut commands: Commands,
    balls: Query<&Transform, (With<Ball>, Without<BallShadow>)>,
    mut shadows: Query<(Entity, &BallShadow, &mut Transform), Without<Ball>>,
) {
    for (entity, shadow, mut transform) in shadows.iter_mut() {
        match balls.get(shadow.ball) {
            Ok(ball_transform) => {
                transform.translation.x = ball_transform.translation.x;
                transform.translation.y = ball_transform.translation.y;
            }
            Err(_) => {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Bakes static drop shadows into the light map: a dark copy of each new
/// [`ShadowCaster`]'s mesh, one per overhead light, offset so it reads as a drop
/// shadow that matches the object's shape. Static, so spawned once.
fn spawn_static_shadows(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
    casters: Query<(&Transform, &Mesh2d, &ShadowCaster), Added<ShadowCaster>>,
) {
    if casters.is_empty() {
        return;
    }
    let material = shadow_material(&mut materials);
    for (transform, mesh, caster) in casters.iter() {
        for dir in SHADOW_DIRS {
            let mut shadow_transform = *transform;
            // Enlarge about the object centre (e.g. so a bumper shadow clears its cap).
            shadow_transform.scale *= caster.scale;
            shadow_transform.translation.x += dir.x * MESH_SHADOW_OFFSET;
            shadow_transform.translation.y += dir.y * MESH_SHADOW_OFFSET;
            shadow_transform.translation.z = SHADOW_Z;
            commands.spawn((
                Name::from("Static shadow"),
                Mesh2d(mesh.0.clone()),
                MeshMaterial2d(material.clone()),
                shadow_transform,
                DespawnOnExit(Screen::Gameplay),
                lightmap_layer(),
            ));
        }
    }
}
