use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::lightmap::lightmap_layer;
use crate::screens::Screen;
use crate::vpx::triangulate::triangulate_polygon;
use bevy::asset::{Asset, Assets, RenderAssetUsages};
use bevy::color::Srgba;
use bevy::ecs::entity::Entities;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::image::Image;
use bevy::mesh::{Indices, Mesh, Mesh2d, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, Extent3d,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension,
    TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use vpin::vpx;
use vpin::vpx::gameitem::light::Fader;
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
/// Z of the animated insert lights on the main view: just above the playfield,
/// below the ball (~0.0125), rubbers and plastics. Inserts emit light, so they
/// draw over the playfield art (and its shadows) but under everything physical.
const INSERT_LIGHT_Z: f32 = 0.001;
/// Additive glow alpha per unit of vpx light intensity, for the steady GI bulbs
/// only. Pinball general illumination uses many low-power bulbs (intensity ~4);
/// scaling keeps that wash gentle. Animated inserts instead use vpinball's own
/// model: the added light is `falloff * intensity` saturated by the framebuffer
/// (ClassicLightShader's `saturate(atten * intensity)`), which is what lights the
/// whole insert at its colour rather than a dim spot in the middle.
const INTENSITY_TO_ALPHA: f32 = 0.1;
/// Cap so no single GI lamp dominates.
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
/// Nominal height the moving casters' shadows project at (metres). A grounded
/// caster's shadow is the sweep from its base (no offset, it touches the caster)
/// to its top; projecting the single silhouette copy at the mid height - the
/// ball's centre, about half a flipper - keeps the near end tucked under the
/// caster instead of floating beside it, while the far edge still peeks out.
const SHADOW_OBJECT_HEIGHT_M: f32 = BALL_RADIUS_M;
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

/// How a light's intensity chases its on/off target, vpinball's `Fader` modes
/// (light.cpp `UpdateAnimation`).
enum LightFader {
    /// Intensity jumps straight to the target.
    None,
    /// Linear ramp at the authored fade speeds.
    Linear,
    /// Tungsten filament: vpinball simulates a BULB_44 bulb (bulb.cpp) whose
    /// visible emission rises steeply then saturates. We approximate that curve
    /// with an exponential approach over the same authored fade time.
    Incandescent,
}

/// vpinball's light animation (light.cpp `UpdateAnimation`/`UpdateBlinker`): a blink
/// pattern advances one character every `interval`, and the intensity chases the
/// pattern's on/off target through the fader. Tables ship their lamps off and a ROM
/// would normally drive them; we do not run scripts, so every insert gets this
/// blinker as attract-style demo behaviour.
#[derive(Component)]
pub(crate) struct LightAnimation {
    /// The blink pattern as on/off frames (vpx `blink_pattern`, '1' = lit).
    pattern: Vec<bool>,
    /// Seconds per pattern frame (vpx `blink_interval`, default 125 ms).
    interval: f32,
    /// Current frame in the pattern.
    frame: usize,
    /// Seconds until the next frame advance. All blinkers start together on one
    /// shared clock, like vpinball's (UpdateBlinker runs off global time): tables
    /// author chases as per-light walking-bit patterns (e.g. a bonus ladder with
    /// `10000000000`, `01000000000`, ...) that only line up when synchronized.
    next_blink: f32,
    fader: LightFader,
    /// Fade rates in intensity per second (vpx stores intensity per ms). May be
    /// infinite (some tables author it so); the clamp to the target handles that.
    fade_up: f32,
    fade_down: f32,
    /// The authored full intensity, the "on" target.
    intensity: f32,
    /// Animated intensity, chasing on/off through the fader; lands in the
    /// material's intensity each frame.
    current: f32,
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

/// Uniform parameters of [`InsertGlowMaterial`].
#[derive(ShaderType, Clone, Debug)]
pub(crate) struct InsertLightParams {
    /// rgb: the light colour at the falloff edge (linear); a: the current
    /// animated intensity in raw vpx units (inserts author 10-90; saturation in
    /// the shader does the rest).
    pub(crate) color: Vec4,
    /// rgb: the light colour at the centre, vpx "color full" (`color2`). vpinball
    /// lerps centre to edge by sqrt of the falloff distance; tables author e.g. a
    /// warm-white centre fading to a near-black rim.
    pub(crate) color_full: Vec4,
    /// The vpx falloff power shaping the attenuation curve (default 2).
    pub(crate) falloff_power: f32,
    /// Playfield extent in world metres (the table is centred on the origin),
    /// to derive table-space art UVs from the fragment's world position.
    pub(crate) table_size: Vec2,
}

/// Material for the animated insert lights, drawn directly over the playfield on
/// the main view like vpinball's classic light (the shape mesh composited onto the
/// already-lit playfield). The shader ports PS_LightWithTexel: the light colour
/// times the falloff attenuation is added over the insert's art, then the art is
/// re-composited with Overlay (darks like decal prints stay dark) and Screen (the
/// art brightens the result) - see ClassicLightShader.hlsl. The light map's
/// multiply could never do this: a dark insert print would stay dark no matter
/// the glow, reading as a dim spot in the middle of the insert.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct InsertGlowMaterial {
    #[uniform(0)]
    params: InsertLightParams,
    /// The art under/of the insert: the light's own image like vpinball, usually
    /// the playfield image.
    #[texture(1)]
    #[sampler(2)]
    art: Handle<Image>,
}

impl Material2d for InsertGlowMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/insert_light.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        // Transparent pass: sorted back to front, so the playfield (opaque) and
        // anything below the insert's z is on screen before the light blends
        // over it.
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Premultiplied: the shader weights the lit pixel by the saturating
        // falloff-times-intensity, crossfading from the unlit framebuffer to the
        // fully lit insert.
        const PREMULTIPLIED: BlendComponent = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        };
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|fragment| fragment.targets.get_mut(0))
            .and_then(|target| target.as_mut())
        {
            target.blend = Some(BlendState {
                color: PREMULTIPLIED,
                alpha: PREMULTIPLIED,
            });
        }
        Ok(())
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<GlowMaterial>::default());
    app.add_plugins(Material2dPlugin::<InsertGlowMaterial>::default());
    app.add_systems(Startup, setup_lighting);
    app.add_systems(
        Update,
        (
            ((spawn_ball_shadows, spawn_flipper_shadows), update_shadows).chain(),
            animate_lights,
        )
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

/// The insert shape mesh, vpinball's classic light geometry (light.cpp
/// `RenderSetup`): the drag-point outline smoothed with the same Catmull-Rom pass
/// as wall meshes, triangulated, centred on the light. The UVs map the radial glow
/// texture over the light's *falloff radius* - exactly vpinball's light shader,
/// which attenuates by `pow(1 - dist / falloff, falloff_power)` with the falloff
/// property as the range (light.cpp `center_range`), NOT the shape extent. Insert
/// shapes are typically well inside their falloff radius, so the whole insert
/// lights near-uniformly and only the rim dims. Our `(1 - d)^2` texture matches
/// the vpx default falloff power 2.
fn insert_mesh(light: &vpx::gameitem::light::Light) -> Option<Mesh> {
    if light.drag_points.len() < 3 {
        return None;
    }
    let smoothed = vpin::vpx::mesh::smooth_drag_points_2d(&light.drag_points, 4.0, true);
    // Offsets from the light centre, in vpx units (y still down like vpx).
    let offsets: Vec<Vec2> = smoothed
        .iter()
        .map(|(x, y)| Vec2::new(x - light.center.x, y - light.center.y))
        .collect();
    let max_dist = offsets.iter().map(|d| d.length()).fold(0.0f32, f32::max);
    if max_dist <= 0.0 {
        return None;
    }
    // The attenuation range: the falloff radius like vpinball, falling back to the
    // shape extent for degenerate authoring. Shape parts beyond the falloff sample
    // past the texture edge, where the clamped texture is transparent - the same
    // zero vpinball's saturate produces there.
    let range = if light.falloff_radius > 0.0 {
        light.falloff_radius
    } else {
        max_dist
    };
    let positions: Vec<[f32; 3]> = offsets
        .iter()
        .map(|d| [vpu_to_m(d.x), -vpu_to_m(d.y), 0.0])
        .collect();
    // Texture centre (0.5, 0.5) on the light centre, texture edge at the falloff
    // radius.
    let uvs: Vec<[f32; 2]> = offsets
        .iter()
        .map(|d| [0.5 + d.x * 0.5 / range, 0.5 + d.y * 0.5 / range])
        .collect();
    let outline: Vec<Vec2> = positions.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    let indices = triangulate_polygon(&outline);
    if indices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

/// An authored per-ms fade speed as intensity per second; zero, negative or NaN
/// (vpx allows even infinity) becomes an infinite rate, i.e. an instant snap.
fn fade_rate(per_ms: f32) -> f32 {
    if per_ms > 0.0 {
        per_ms * 1000.0
    } else {
        f32::INFINITY
    }
}

/// The blinker + fader state for one insert, from its authored vpx fields
/// (vpinball's defaults where unset: pattern "10", interval 125 ms, linear fader).
fn light_animation(light: &vpx::gameitem::light::Light) -> LightAnimation {
    let mut pattern: Vec<bool> = light.blink_pattern.chars().map(|c| c == '1').collect();
    if pattern.is_empty() {
        pattern = vec![true, false];
    }
    let interval = if light.blink_interval > 0 {
        light.blink_interval as f32 / 1000.0
    } else {
        0.125
    };
    let fader = match light.fader {
        Some(Fader::None) => LightFader::None,
        Some(Fader::Incandescent) => LightFader::Incandescent,
        // vpinball defaults to the linear fader (light.h `m_fader`).
        Some(Fader::Linear) | None => LightFader::Linear,
    };
    LightAnimation {
        next_blink: interval,
        pattern,
        interval,
        frame: 0,
        fader,
        // Per-ms authored speeds to per-second. A missing/zero speed degenerates
        // to a snap (infinite ramp clamps straight to the target).
        fade_up: fade_rate(light.fade_speed_up),
        fade_down: fade_rate(light.fade_speed_down),
        intensity: light.intensity,
        current: 0.0,
    }
}

pub(super) fn spawn_light(
    meshes: &mut ResMut<Assets<Mesh>>,
    glow_materials: &mut ResMut<Assets<GlowMaterial>>,
    insert_materials: &mut ResMut<Assets<InsertGlowMaterial>>,
    glow_texture: &Handle<Image>,
    vpx_asset: &crate::vpx::VpxAsset,
    table_size_m: Vec2,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    light: &vpx::gameitem::light::Light,
) {
    // Backglass-mode lights belong on the backdrop, not the playfield.
    if light.visible == Some(false) || light.is_backglass {
        return;
    }
    // GI bulbs (general illumination, on whenever the table is powered) glow
    // steadily. Everything else is an insert or feature lamp a ROM would drive;
    // we do not run scripts, so they all get the blinker animation instead
    // (attract-style demo of vpinball's light animation).
    let is_gi = light.name.to_lowercase().contains("gi");
    // Every light renders its drag-point shape: vpinball draws the same shape
    // mesh for classic inserts and bulb lights alike (m_lightmapMeshBuffer in
    // light.cpp Render; only the shader differs), so a bulb-light insert still
    // fills its authored insert outline. Shapeless/degenerate lights fall back
    // to a glow disc over the falloff radius.
    let mesh = insert_mesh(light)
        .map(|mesh| meshes.add(mesh))
        .unwrap_or_else(|| {
            // Cover the falloff radius, but never collapse to nothing for lights
            // that only define a small core.
            let radius = vpu_to_m(light.falloff_radius).max(vpu_to_m(light.mesh_radius));
            meshes.add(Circle::new(radius))
        });
    let base = (
        Light {
            name: light.name.clone(),
        },
        Name::from(format!("Light {}", light.name)),
        Mesh2d(mesh),
    );
    let translation = Vec2::new(
        vpx_to_bevy_transform.translation.x + vpu_to_m(light.center.x),
        vpx_to_bevy_transform.translation.y - vpu_to_m(light.center.y),
    );
    let color = Color::from(Srgba::rgb_u8(light.color.r, light.color.g, light.color.b)).to_linear();
    if is_gi {
        // GI washes the playfield through the light map, tinted by the light
        // colour with a gentle intensity-scaled alpha so the many low-power
        // bulbs stay soft.
        let alpha = (light.intensity * INTENSITY_TO_ALPHA).clamp(0.0, MAX_GLOW_ALPHA);
        parent.spawn((
            base,
            Transform::from_translation(translation.extend(LIGHT_Z)),
            MeshMaterial2d(glow_materials.add(GlowMaterial {
                color: color.with_alpha(alpha),
                texture: glow_texture.clone(),
            })),
            // Render into the light map only, not directly on screen.
            lightmap_layer(),
        ));
    } else {
        // Animated lamps composite directly over the playfield on the main view,
        // like vpinball's classic light render; they start dark and the
        // animation drives the params alpha with the raw vpx intensity. The art
        // the shader re-composites is the light's own image like vpinball,
        // usually the playfield image.
        let art = vpx_asset
            .image(light.image.as_str())
            .or_else(|| vpx_asset.image(vpx_asset.raw.gamedata.image.as_str()))
            .cloned()
            .unwrap_or_default();
        let color_full = Color::from(Srgba::rgb_u8(
            light.color2.r,
            light.color2.g,
            light.color2.b,
        ))
        .to_linear();
        // vpinball's no-contribution early-out: a light with both colours black
        // adds nothing and is not drawn at all (light.cpp Render).
        if color.red == 0.0
            && color.green == 0.0
            && color.blue == 0.0
            && color_full.red == 0.0
            && color_full.green == 0.0
            && color_full.blue == 0.0
        {
            return;
        }
        parent.spawn((
            base,
            Transform::from_translation(translation.extend(INSERT_LIGHT_Z)),
            MeshMaterial2d(insert_materials.add(InsertGlowMaterial {
                params: InsertLightParams {
                    color: Vec4::new(color.red, color.green, color.blue, 0.0),
                    color_full: Vec4::new(color_full.red, color_full.green, color_full.blue, 0.0),
                    // vpx defaults the falloff power to 2; guard degenerate 0,
                    // which would make pow() a hard-edged disc.
                    falloff_power: if light.falloff_power > 0.0 {
                        light.falloff_power
                    } else {
                        2.0
                    },
                    table_size: table_size_m,
                },
                art,
            })),
            light_animation(light),
        ));
    }
}

/// Advances every animated light, vpinball's light.cpp `UpdateAnimation`: the
/// blinker picks the on/off target from the pattern, the fader moves the intensity
/// toward it, and the result lands in the glow material's alpha.
fn animate_lights(
    time: Res<Time>,
    mut glow_materials: ResMut<Assets<InsertGlowMaterial>>,
    mut lights: Query<(&mut LightAnimation, &MeshMaterial2d<InsertGlowMaterial>)>,
) {
    let dt = time.delta_secs();
    for (mut anim, material) in &mut lights {
        // The blinker (light.h UpdateBlinker): one pattern frame per interval.
        anim.next_blink -= dt;
        while anim.next_blink <= 0.0 {
            anim.frame = (anim.frame + 1) % anim.pattern.len();
            anim.next_blink += anim.interval;
        }
        let target = if anim.pattern[anim.frame] {
            anim.intensity
        } else {
            0.0
        };
        anim.current = match anim.fader {
            LightFader::None => target,
            LightFader::Linear => {
                // Authored ramp speed, clamped at the target. An infinite or zero
                // authored speed degenerates to a snap.
                if anim.current < target {
                    (anim.current + anim.fade_up * dt).min(target)
                } else {
                    (anim.current - anim.fade_down * dt).max(target)
                }
            }
            LightFader::Incandescent => {
                // Exponential stand-in for vpinball's filament sim: vpinball maps
                // the authored fade time onto the BULB_44 heat-up (full power in
                // 30-40 ms of sim time), so the bulb settles within roughly the
                // authored time with a steep start - which is exactly an
                // exponential with tau of a quarter of that time.
                let rate = if anim.current < target {
                    anim.fade_up
                } else {
                    anim.fade_down
                };
                if rate > 0.0 && rate.is_finite() {
                    let fade_time = anim.intensity / rate;
                    let tau = (fade_time / 4.0).max(1e-4);
                    anim.current + (target - anim.current) * (1.0 - (-dt / tau).exp())
                } else {
                    target
                }
            }
        };
        if let Some(glow) = glow_materials.get_mut(&material.0) {
            // The raw intensity goes to the shader (inserts author 10-90);
            // vpinball's saturate(atten * intensity) there lights the whole
            // insert at full colour with only the falloff rim dimming.
            glow.params.color.w = anim.current;
        }
    }
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
