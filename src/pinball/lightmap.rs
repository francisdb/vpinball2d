//! Offscreen light/shadow map composited onto the playfield.
//!
//! A dedicated camera renders only the light/shadow layer ([`LIGHTMAP_LAYER`]) over
//! the exact playfield rect into an offscreen texture. The playfield is then drawn
//! with [`PlayfieldLightMaterial`], which multiplies the table image by that map.
//!
//! Because the map covers exactly the playfield, lighting is clipped to the
//! playfield by construction: glows never bleed past the edge (so no matte is
//! needed, and the backglass margins stay visible), and static objects can drop
//! shadows into the map.

use bevy::asset::{Asset, Assets};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Camera, ClearColorConfig, RenderTarget, ScalingMode};
use bevy::image::Image;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::sprite_render::Material2d;
use bevy::sprite_render::Material2dPlugin;

/// Render layer that the light/shadow map camera sees. Light glows and shadows live
/// here only, so the main camera does not draw them directly; they reach the screen
/// only through the playfield composite.
pub(crate) const LIGHTMAP_LAYER: usize = 1;

/// Render layer of the static-shadow pass: every static table item that should cast
/// a shadow renders to this layer *as well as* the main view, into an offscreen
/// image with a transparent background (no playfield, ball or flippers). The light
/// map then darkens the playfield with that image projected away from each lamp
/// (see `pinball::light`), so shadows are per pixel exactly what the table shows:
/// cut-out plastics cast their art, screws on a plastic merge into its shadow.
pub(crate) const STATIC_SHADOW_LAYER: usize = 2;

/// The light map is cleared to this ambient level; the playfield reads as
/// `table_image * ambient` where nothing lights it, brighter where lit, darker
/// where shadowed. Higher = brighter unlit playfield, but less headroom before lit
/// areas clip (HDR + bloom later would remove that ceiling).
const AMBIENT: Color = Color::srgb(0.7, 0.7, 0.7);

/// Light map texture height in pixels; width is derived from the table aspect.
/// Driven by the single shadow-softness knob in `light` (a low value blurs the
/// glows and shadows into soft edges via the linear upscale onto the playfield).
const LIGHTMAP_HEIGHT_PX: u32 = crate::pinball::light::SHADOW_SOFTNESS_PX;

/// Marks the offscreen light map camera, so systems that operate on the main view
/// camera (cursor picking, nudge, projection) can exclude it with `Without`.
#[derive(Component)]
pub(crate) struct LightmapCamera;

/// Material for the playfield: multiplies the table image by the light map.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct PlayfieldLightMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub(crate) playfield: Handle<Image>,
    #[texture(2)]
    #[sampler(3)]
    pub(crate) light_map: Handle<Image>,
}

impl Material2d for PlayfieldLightMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/playfield_light.wgsl".into()
    }
}

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<PlayfieldLightMaterial>::default());
}

/// The render layer for everything that should be baked into the light map.
pub(crate) fn lightmap_layer() -> RenderLayers {
    RenderLayers::layer(LIGHTMAP_LAYER)
}

/// The render layers of a static item that casts a shadow: drawn by the main camera
/// and again by the static-shadow camera. Render layers do not propagate to
/// children, so every visual entity of the item needs this.
pub(crate) fn casts_shadow_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, STATIC_SHADOW_LAYER])
}

/// Camera that renders the static-shadow layer over the playfield rect into the
/// given image, on a fully transparent background. Orders before the lightmap
/// camera, which composites the result as the static shadows.
pub(crate) fn static_shadow_camera(
    target: Handle<Image>,
    table_width_m: f32,
    table_depth_m: f32,
) -> impl Bundle {
    (
        Name::from("Static shadow camera"),
        Camera2d,
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(target.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: table_width_m,
                height: table_depth_m,
            },
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(STATIC_SHADOW_LAYER),
    )
}

/// Create the offscreen light map texture, sized to the table aspect so texels are
/// roughly square.
pub(crate) fn lightmap_image(
    images: &mut Assets<Image>,
    table_width_m: f32,
    table_depth_m: f32,
) -> Handle<Image> {
    let width_px = ((LIGHTMAP_HEIGHT_PX as f32) * table_width_m / table_depth_m).round() as u32;
    images.add(Image::new_target_texture(
        width_px.max(1),
        LIGHTMAP_HEIGHT_PX,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ))
}

/// Camera that renders the light/shadow layer over the playfield rect into the
/// light map. Orders before the main camera so the map is ready when the playfield
/// samples it.
pub(crate) fn lightmap_camera(
    light_map: Handle<Image>,
    table_width_m: f32,
    table_depth_m: f32,
) -> impl Bundle {
    (
        Name::from("Lightmap Camera"),
        LightmapCamera,
        Camera2d,
        Camera {
            order: -1,
            clear_color: ClearColorConfig::Custom(AMBIENT),
            ..default()
        },
        RenderTarget::Image(light_map.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: table_width_m,
                height: table_depth_m,
            },
            ..OrthographicProjection::default_2d()
        }),
        lightmap_layer(),
    )
}
