use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::lightmap::lightmap_layer;
use crate::screens::Screen;
use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::color::Srgba;
use bevy::ecs::entity::Entities;
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

/// Z of the light glows in the light map: below the shadows. Glows are additive, so
/// drawn above a shadow they add the brightness right back and the shadow vanishes
/// wherever GI glows cover the playfield; drawn first, the shadow's dark multiply
/// attenuates ambient and glow alike. The vpx bulb height is irrelevant here.
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
/// Z of the shadows in the light map: above the glows (see [`LIGHT_Z`]).
const SHADOW_Z: f32 = 0.012;
/// Nominal height of the moving shadow casters (metres): the ball's diameter, also
/// about a flipper's height. With the lamp height it sets how far their shadows
/// stretch away from the lamp.
const SHADOW_OBJECT_HEIGHT_M: f32 = 2.0 * BALL_RADIUS_M;
/// Nominal height of the static decor for the static-shadow pass (vpx units): the
/// pass mixes playfield-level posts with raised plastics, and the plastics tops
/// (~65 vpu) dominate what the eye reads, so their shadows get the honest length.
const STATIC_SHADOW_HEIGHT_VPU: f32 = 65.0;
/// Fallback lamp height when a table has no usable `light_height` (vpx units).
const DEFAULT_LAMP_HEIGHT_VPU: f32 = 5000.0;
/// Tables hang their lights very high (typically 5000 vpu, ~2.7 m), which makes the
/// two shadows short and nearly coincident. Bring the lamps down by this factor so
/// the double shadows read distinctly, while taller-lit tables still differ.
const LAMP_HEIGHT_SCALE: f32 = 0.25;

/// The two overhead lamps every shadow is cast from, vpinball's scene lights: on the
/// table centre line at 1/3 and 2/3 of its depth, at the table's `light_height`
/// (`Renderer.cpp`, `m_Light[0..1]`). Built per table and inserted at level spawn.
#[derive(Resource)]
pub(crate) struct OverheadLights {
    lamps: [Vec2; 2],
    /// How far an object's top is dragged away from the lamp per metre of horizontal
    /// distance: `h / (H - h)` for the nominal object height `h` and lamp height `H`.
    /// One value for the moving casters (ball height), one for the static pass
    /// (plastics height).
    stretch: f32,
    static_stretch: f32,
}

impl OverheadLights {
    /// The lamps for a table (world coordinates, table centred on the origin),
    /// `light_height_vpu` from the table's gamedata.
    pub(crate) fn for_table(table_depth_m: f32, light_height_vpu: f32) -> Self {
        let height_m = vpu_to_m(
            if light_height_vpu > 0.0 {
                light_height_vpu
            } else {
                DEFAULT_LAMP_HEIGHT_VPU
            } * LAMP_HEIGHT_SCALE,
        );
        let static_height_m = vpu_to_m(STATIC_SHADOW_HEIGHT_VPU);
        Self {
            lamps: [
                Vec2::new(0.0, table_depth_m / 6.0),
                Vec2::new(0.0, -table_depth_m / 6.0),
            ],
            stretch: SHADOW_OBJECT_HEIGHT_M / (height_m - SHADOW_OBJECT_HEIGHT_M).max(0.1),
            static_stretch: static_height_m / (height_m - static_height_m).max(0.1),
        }
    }

    /// A world-space point projected away from the given lamp: where the shadow of
    /// an object top at `pos` lands. The single place the shadow projection lives.
    fn project(&self, lamp: usize, pos: Vec2) -> Vec2 {
        pos + (pos - self.lamps[lamp]) * self.stretch
    }

    /// World translation of the shadow of a compact object at `pos` (the ball, a
    /// flipper around its pivot), cast by the given lamp.
    fn shadow_translation(&self, lamp: usize, pos: Vec2) -> Vec3 {
        self.project(lamp, pos).extend(SHADOW_Z)
    }
}

/// The two quads that composite the static-shadow render into the light map: the
/// table's static items rendered on a transparent background (see
/// `lightmap::static_shadow_camera`), darkened and scaled about each lamp. The
/// projection `p + (p - lamp) * stretch` is a uniform scale about the lamp, so
/// scaling the whole image casts every pixel exactly: cut-out plastics cast their
/// art, screws on a plastic merge into the plastic's own shadow.
pub(crate) fn static_shadow_quads(
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    lights: &OverheadLights,
    static_render: Handle<Image>,
    table_width_m: f32,
    table_depth_m: f32,
) -> [impl Bundle; 2] {
    let mesh = meshes.add(Rectangle::new(table_width_m, table_depth_m));
    let material = materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, per_lamp_shadow_alpha()),
        alpha_mode: AlphaMode2d::Blend,
        texture: Some(static_render),
        ..default()
    });
    [0, 1].map(|lamp| {
        let stretch = lights.static_stretch;
        (
            Name::from("Static shadows"),
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material.clone()),
            Transform {
                // Scaling about the origin then shifting by -lamp * stretch equals
                // scaling about the lamp: quad point p lands on project(lamp, p).
                translation: (-lights.lamps[lamp] * stretch).extend(SHADOW_Z),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(1.0 + stretch, 1.0 + stretch, 1.0),
            },
            lightmap_layer(),
            DespawnOnExit(Screen::Gameplay),
        )
    })
}

#[derive(Component)]
pub struct Light {
    #[allow(dead_code)]
    pub name: String,
}

/// One drop shadow in the light map: a dark silhouette following its source entity,
/// cast away from one overhead lamp. The moving parts (ball, flippers) get these;
/// static items cast through the static-shadow render pass instead (see
/// `static_shadow_quads`). Shadows are separate entities, not children: a child's
/// lamp offset would rotate along with its parent (as if the lamp moved), and the
/// ball's spin must not spin its blob.
#[derive(Component)]
struct Shadow {
    /// The entity whose silhouette this is.
    source: Entity,
    /// Which overhead lamp casts it.
    lamp: usize,
    /// Whether the silhouette follows the source's rotation (flippers do; the
    /// ball's enlarged disc is rotation-independent).
    follow_rotation: bool,
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
        ((spawn_ball_shadows, spawn_flipper_shadows), update_shadows)
            .chain()
            .run_if(in_state(Screen::Gameplay)),
    );
}

fn setup_lighting(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    // Soft glow: square the falloff for a gentle, diffuse light.
    let glow = images.add(radial_image(|distance| {
        (1.0 - distance).clamp(0.0, 1.0).powf(2.0)
    }));
    commands.insert_resource(LightingAssets { glow });
}

/// The darkness of a single lamp's shadow copy. Every caster shades once per lamp
/// and the copies largely overlap (fully so right under the ball), so the per-copy
/// strength is set to compound to [`SHADOW_ALPHA`] where both overlap (the umbra);
/// where only one lamp is blocked the penumbra reads about half as strong.
fn per_lamp_shadow_alpha() -> f32 {
    1.0 - (1.0 - SHADOW_ALPHA).sqrt()
}

/// The flat-dark material every shadow uses; its soft look comes entirely from the
/// low-resolution light map, so ball and static shadows match.
fn shadow_material(materials: &mut Assets<ColorMaterial>) -> Handle<ColorMaterial> {
    materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, per_lamp_shadow_alpha()),
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

/// Spawns the [`Shadow`] entities of one source: a dark copy of its silhouette per
/// overhead lamp, starting at the source's current pose. Like the static-shadow
/// quads, the silhouette is enlarged by the projection factor `1 + stretch`, so the
/// ball's shadow derives from its own disc exactly as a rubber's derives from its
/// ring - the same rule everywhere.
fn spawn_shadows_for(
    commands: &mut Commands,
    lights: &OverheadLights,
    material: &Handle<ColorMaterial>,
    source: Entity,
    mesh: Handle<Mesh>,
    source_transform: &Transform,
    follow_rotation: bool,
) {
    let pos = source_transform.translation.truncate();
    let scale = 1.0 + lights.stretch;
    for lamp in 0..2 {
        let mut transform = Transform::from_translation(lights.shadow_translation(lamp, pos))
            .with_scale(source_transform.scale * Vec3::new(scale, scale, 1.0));
        if follow_rotation {
            transform.rotation = source_transform.rotation;
        }
        commands.spawn((
            Name::from("Shadow"),
            Shadow {
                source,
                lamp,
                follow_rotation,
            },
            Mesh2d(mesh.clone()),
            MeshMaterial2d(material.clone()),
            transform,
            lightmap_layer(),
            DespawnOnExit(Screen::Gameplay),
        ));
    }
}

/// Spawns each ball's drop shadows: an enlarged disc silhouette per lamp.
fn spawn_ball_shadows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    lights: Res<OverheadLights>,
    balls: Query<(Entity, &Transform), Added<Ball>>,
) {
    for (ball, transform) in &balls {
        let mesh = meshes.add(Circle::new(BALL_RADIUS_M));
        let material = shadow_material(&mut materials);
        spawn_shadows_for(
            &mut commands,
            &lights,
            &material,
            ball,
            mesh,
            transform,
            false,
        );
    }
}

/// Spawns each flipper's drop shadows: its outline silhouette per lamp, following
/// the bat's swing.
fn spawn_flipper_shadows(
    mut commands: Commands,
    mut materials: ResMut<Assets<ColorMaterial>>,
    lights: Res<OverheadLights>,
    flippers: Query<(Entity, &Mesh2d, &Transform), Added<crate::pinball::flipper::Flipper>>,
) {
    if flippers.is_empty() {
        return;
    }
    let material = shadow_material(&mut materials);
    for (flipper, mesh, transform) in &flippers {
        spawn_shadows_for(
            &mut commands,
            &lights,
            &material,
            flipper,
            mesh.0.clone(),
            transform,
            true,
        );
    }
}

/// Re-aims every shadow whose source moved: the silhouette at the source's current
/// pose, pushed away from the shadow's lamp at that spot. Sources that disappeared
/// (e.g. a drained ball) take their shadows with them; unmoved sources (static
/// decor, a resting flipper) cost nothing.
fn update_shadows(
    mut commands: Commands,
    lights: Res<OverheadLights>,
    entities: &Entities,
    moved: Query<&Transform, (Changed<Transform>, Without<Shadow>)>,
    mut shadows: Query<(Entity, &Shadow, &mut Transform)>,
) {
    for (entity, shadow, mut transform) in &mut shadows {
        if !entities.contains(shadow.source) {
            commands.entity(entity).despawn();
            continue;
        }
        let Ok(source) = moved.get(shadow.source) else {
            continue;
        };
        let pos = source.translation.truncate();
        transform.translation = lights.shadow_translation(shadow.lamp, pos);
        if shadow.follow_rotation {
            transform.rotation = source.rotation;
        }
    }
}
