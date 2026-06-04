use crate::pinball::ball::{BALL_RADIUS_M, Ball};
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
    /// Harder radial gradient with a solid core and a quick fade, tinted dark for
    /// ball shadows so they read clearly without being soft and washed out.
    shadow: Handle<Image>,
}

/// Radius of a ball drop shadow blob relative to the ball.
const SHADOW_RADIUS: f32 = BALL_RADIUS_M * 1.6;
/// How dark each shadow blob is at most.
const SHADOW_ALPHA: f32 = 0.45;
/// Z of the light glows: a flat surface effect just above the playfield, below
/// the ball shadows and the ball. The vpx bulb height is irrelevant for 2D layering.
const LIGHT_Z: f32 = 0.005;
/// Additive glow alpha per unit of vpx light intensity. Pinball general
/// illumination uses many low-power bulbs (intensity ~4); inserts are far
/// brighter (~90). Scaling by intensity keeps GI gentle while lit inserts stand
/// out. Halved from a brighter baseline so the many overlapping GI bulbs do not
/// overbrighten.
const INTENSITY_TO_ALPHA: f32 = 0.02;
/// Cap so no single lamp blows the playfield to white on the LDR pipeline.
const MAX_GLOW_ALPHA: f32 = 0.6;
/// Z of the ball shadows: above the light glows, just below the ball itself.
const SHADOW_Z: f32 = 0.012;
/// One soft shadow per overhead light, offset away from that light. With the two
/// lights roughly overhead, the offsets are small and point "down" the table.
const SHADOW_OFFSETS: [Vec2; 2] = [
    Vec2::new(-BALL_RADIUS_M * 0.5, -BALL_RADIUS_M * 0.7),
    Vec2::new(BALL_RADIUS_M * 0.5, -BALL_RADIUS_M * 0.7),
];

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
        (spawn_ball_shadows, update_ball_shadows)
            .chain()
            .run_if(in_state(Screen::Gameplay)),
    );
}

fn setup_lighting(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Soft glow: square the falloff for a gentle, diffuse light.
    let glow = images.add(radial_image(|distance| {
        (1.0 - distance).clamp(0.0, 1.0).powf(2.0)
    }));
    // Hard shadow: solid core out to half the radius, then a quick linear fade.
    let shadow = images.add(radial_image(|distance| {
        ((1.0 - distance) * 2.0).clamp(0.0, 1.0)
    }));
    commands.insert_resource(LightingAssets { glow, shadow });
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
    ));
}

// TODO: flippers also need a dynamic drop shadow that tracks their rotation, the
// same way the ball gets one here. The flipper bat is a moving body, so its shadow
// has to follow both position and angle (offset per overhead light).

/// Spawns one drop shadow per ball: a small group of soft dark blobs, one offset
/// per overhead light. Reuses the shared glow texture, tinted dark.
fn spawn_ball_shadows(
    mut commands: Commands,
    lighting: Res<LightingAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    balls: Query<(Entity, &Transform), Added<Ball>>,
) {
    for (ball, ball_transform) in balls.iter() {
        let shadow_mesh = meshes.add(Circle::new(SHADOW_RADIUS));
        let shadow_material = materials.add(ColorMaterial {
            color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
            alpha_mode: AlphaMode2d::Blend,
            texture: Some(lighting.shadow.clone()),
            ..default()
        });
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
                    MeshMaterial2d(shadow_material.clone()),
                    Transform::from_xyz(SHADOW_OFFSETS[0].x, SHADOW_OFFSETS[0].y, 0.0),
                ),
                (
                    Mesh2d(shadow_mesh),
                    MeshMaterial2d(shadow_material),
                    Transform::from_xyz(SHADOW_OFFSETS[1].x, SHADOW_OFFSETS[1].y, 0.0),
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
