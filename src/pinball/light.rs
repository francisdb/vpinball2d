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
/// Ball shadow silhouette radius: the ball's disc, enlarged so the shadow peeks out
/// from under it.
const BALL_SHADOW_RADIUS: f32 = BALL_RADIUS_M * 1.6;
/// Nominal height of the shadow-casting silhouettes (metres): a ball diameter, which
/// is also about the height of posts, flippers and plastics. With the lamp height it
/// sets how far a shadow stretches away from the lamp.
const SHADOW_OBJECT_HEIGHT_M: f32 = 2.0 * BALL_RADIUS_M;
/// Fallback lamp height when a table has no usable `light_height` (vpx units).
const DEFAULT_LAMP_HEIGHT_VPU: f32 = 5000.0;
/// Items whose base sits at or above this height (vpx units) float over other
/// geometry (screws fastening a plastic, score cards on the apron) rather than
/// standing on the playfield.
const SHADOW_BASE_MAX_VPU: f32 = 50.0;

/// Whether an item with its base at this height (vpx units) drops a shadow into the
/// playfield light map. The map only represents the playfield surface, so an item
/// standing on raised geometry must not shade the playfield underneath it.
pub(crate) fn casts_playfield_shadow(base_height_vpu: f32) -> bool {
    base_height_vpu < SHADOW_BASE_MAX_VPU
}

/// A raised item only casts when its footprint covers at least this fraction of the
/// table: plastics sheets and panels qualify (North Pole's smallest panel is ~0.4%),
/// while small raised decor (fastening screws on a plastic, ~0.01%) must not print
/// through whatever it stands on.
pub(crate) const RAISED_MIN_TABLE_FRACTION: f32 = 0.002;

/// Fraction of the table area covered by the bounding box of the given outline
/// points (vpx units), widened by `expand` on every side (e.g. a ramp's half width).
pub(crate) fn footprint_fraction(
    points: impl Iterator<Item = (f32, f32)>,
    expand: f32,
    gamedata: &vpx::gamedata::GameData,
) -> f32 {
    let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    let mut any = false;
    for (x, y) in points {
        any = true;
        min = min.min(Vec2::new(x, y));
        max = max.max(Vec2::new(x, y));
    }
    if !any {
        return 0.0;
    }
    let size = max - min + Vec2::splat(2.0 * expand);
    let table = (gamedata.right - gamedata.left) * (gamedata.bottom - gamedata.top);
    if table > 0.0 {
        (size.x * size.y) / table
    } else {
        0.0
    }
}
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
    stretch: f32,
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
        Self {
            lamps: [
                Vec2::new(0.0, table_depth_m / 6.0),
                Vec2::new(0.0, -table_depth_m / 6.0),
            ],
            stretch: SHADOW_OBJECT_HEIGHT_M / (height_m - SHADOW_OBJECT_HEIGHT_M).max(0.1),
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

    /// The shadow mesh of a static caster: its mesh taken to world space, enlarged
    /// about its centre by `scale`, with every vertex projected away from the lamp.
    /// Per vertex, because a caster's mesh can span much of the table (a rubber
    /// ring, a long wall) while its entity transform may sit far from the geometry;
    /// a single per-entity offset would aim parts of such a shadow the wrong way.
    fn project_shadow_mesh(
        &self,
        mesh: &Mesh,
        world: &Transform,
        scale: f32,
        lamp: usize,
    ) -> Option<Mesh> {
        let points: Vec<Vec2> = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)?
            .as_float3()?
            .iter()
            .map(|p| world.transform_point(Vec3::from(*p)).truncate())
            .collect();
        let (min, max) = points.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(min, max), p| (min.min(*p), max.max(*p)),
        );
        let center = (min + max) * 0.5;
        let positions: Vec<[f32; 3]> = points
            .iter()
            .map(|p| {
                let enlarged = center + (*p - center) * scale;
                let projected = self.project(lamp, enlarged);
                [projected.x, projected.y, 0.0]
            })
            .collect();
        let mut shadow = Mesh::new(
            bevy::mesh::PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        shadow.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        if let Some(uvs) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            shadow.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
        }
        if let Some(indices) = mesh.indices() {
            shadow.insert_indices(indices.clone());
        }
        Some(shadow)
    }
}

#[derive(Component)]
pub struct Light {
    #[allow(dead_code)]
    pub name: String,
}

/// One drop shadow in the light map: a dark silhouette following its source entity,
/// cast away from one overhead lamp. Every shadow - ball, flipper, static decor -
/// is one of these, sharing the same lamp model, offset, material and update path.
/// Shadows are separate entities, not children: a child's lamp offset would rotate
/// along with its parent (as if the lamp moved), and the ball's spin must not spin
/// its blob.
#[derive(Component)]
struct Shadow {
    /// The entity whose silhouette this is.
    source: Entity,
    /// Which overhead lamp casts it.
    lamp: usize,
    /// Whether the silhouette follows the source's rotation (flippers and static
    /// meshes do; the ball's enlarged disc is rotation-independent).
    follow_rotation: bool,
}

/// Marks a static object that drops a shadow into the light map. A dark copy of the
/// object's mesh is rendered into the map, offset per overhead light, so the shadow
/// matches the object's shape. `scale` enlarges the shadow about the object centre
/// (e.g. so a bumper's shadow clears its wider cap); use 1.0 for a 1:1 copy. Spawn
/// the object as a direct child of the level (so its `Transform` is world space) for
/// the shadow to land in the right place.
///
/// `texture` makes the shadow a cut-out: the dark copy is multiplied by the texture,
/// so only its opaque areas cast (a raised plastics sheet shades the playfield with
/// the shape of its printed art, not its full polygon).
#[derive(Component, Clone)]
pub(crate) struct ShadowCaster {
    pub(crate) scale: f32,
    pub(crate) texture: Option<Handle<Image>>,
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
            (
                spawn_ball_shadows,
                spawn_flipper_shadows,
                spawn_static_shadows,
            ),
            update_shadows,
        )
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

/// The flat-dark material every shadow uses; its soft look comes entirely from the
/// low-resolution light map, so ball and static shadows match.
fn shadow_material(materials: &mut Assets<ColorMaterial>) -> Handle<ColorMaterial> {
    materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    })
}

/// The cut-out variant: the dark shadow colour multiplied by a texture, so only the
/// texture's opaque areas darken the light map (a plastics sheet casts its printed
/// art, the holes around each plastic cast nothing).
fn cut_out_shadow_material(
    materials: &mut Assets<ColorMaterial>,
    texture: Handle<Image>,
) -> Handle<ColorMaterial> {
    materials.add(ColorMaterial {
        color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
        alpha_mode: AlphaMode2d::Blend,
        texture: Some(texture),
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
/// overhead lamp, starting at the source's current pose.
#[allow(clippy::too_many_arguments)]
fn spawn_shadows_for(
    commands: &mut Commands,
    lights: &OverheadLights,
    material: &Handle<ColorMaterial>,
    source: Entity,
    mesh: Handle<Mesh>,
    source_transform: &Transform,
    scale: f32,
    follow_rotation: bool,
) {
    let pos = source_transform.translation.truncate();
    for lamp in 0..2 {
        let mut transform = Transform::from_translation(lights.shadow_translation(lamp, pos))
            // Enlarge about the source centre (e.g. so a bumper shadow clears its cap).
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
        let mesh = meshes.add(Circle::new(BALL_SHADOW_RADIUS));
        let material = shadow_material(&mut materials);
        spawn_shadows_for(
            &mut commands,
            &lights,
            &material,
            ball,
            mesh,
            transform,
            1.0,
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
            1.0,
            true,
        );
    }
}

/// Spawns the drop shadows of each new [`ShadowCaster`]: its mesh projected away
/// from each lamp, baked in world space (static casters never move). The projection
/// is per vertex (see [`OverheadLights::project_shadow_mesh`]): a caster's entity
/// transform often anchors at the table corner with the real geometry in the mesh
/// (walls, rubbers, ramps), so a per-entity offset would cast from the wrong spot.
fn spawn_static_shadows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    lights: Res<OverheadLights>,
    casters: Query<(&Transform, &Mesh2d, &ShadowCaster), Added<ShadowCaster>>,
) {
    if casters.is_empty() {
        return;
    }
    let solid = shadow_material(&mut materials);
    for (transform, mesh, shadow_caster) in &casters {
        let material = match &shadow_caster.texture {
            Some(texture) => cut_out_shadow_material(&mut materials, texture.clone()),
            None => solid.clone(),
        };
        for lamp in 0..2 {
            let Some(shadow) = meshes
                .get(&mesh.0)
                .and_then(|m| lights.project_shadow_mesh(m, transform, shadow_caster.scale, lamp))
            else {
                continue;
            };
            commands.spawn((
                Name::from("Static shadow"),
                Mesh2d(meshes.add(shadow)),
                MeshMaterial2d(material.clone()),
                Transform::from_xyz(0.0, 0.0, SHADOW_Z),
                lightmap_layer(),
                DespawnOnExit(Screen::Gameplay),
            ));
        }
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
