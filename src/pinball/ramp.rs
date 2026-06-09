//! Ramps, rendered as their top-down silhouette.
//!
//! VPinball ramps are 3D (a curved floor with side walls, or a set of wires). This 2D
//! top-down view draws the floor band (flat ramps) or wire ribbons (wire ramps); the
//! mesh is generated at load time (see `crate::vpx::ramp_mesh`). Physics in this world
//! is single-plane, so a ramp's floor is *not* a solid collider - see the module notes
//! below for how ramps interact with the ball.
//!
//! ## Enabling ramps in a 2D world (design notes)
//!
//! A real ramp lifts the ball above the playfield and carries it over other items. A
//! single-plane sim cannot reproduce height, so we project to 2D in stages:
//!
//! 1. Visual (done here). The floor band / wire ribbons are drawn at the ramp's height
//!    as the render z, so a higher ramp layers over lower geometry. The floor is *not*
//!    a solid collider - in 2D that would wall off its whole footprint, which is wrong:
//!    the ball rides *on* the floor, it doesn't bounce off it edge-on.
//!
//! 2. Guide collision (done, see `is_guide` + `spawn_ramp`). A thin ramp acts as a wall
//!    in 2D: the ball cannot pass it. We give such ramps a solid trimesh collider, gated
//!    by the same height check `spawn_wall` uses (`height_bottom` within the ball's
//!    reach), so a guide lifted out of reach stays visual-only. "Thin" means a one-wire
//!    ramp (a single guide wire, e.g. North Pole's `MetalGuide002` arcing over the lane
//!    at ~16mm) or a narrow flat rail (e.g. `MetalGuide001`/`3`). Multi-wire habitrails
//!    and wide flat floors carry the ball, so they are not walls (see stage 3).
//!
//! 3. Elevated transit (open problem). TNA's `Ramp13` shooter feed carries the plunged
//!    ball up the right lane and over `MetalWall001` (the loop rail) into play. In 3D the
//!    ball is simply above the rail; in 2D the rail is in the way. The existing
//!    `Launching` marker already passes the ball through walls until it clears the lane,
//!    which covers this specific feed. A general model (a ball "bound" to a ramp ignores
//!    lower-height colliders along that ramp's footprint until it exits) would generalise
//!    it, but is larger than this change.

use crate::pinball::ball::BALL_RADIUS_M;
use crate::pinball::wall::mesh_collider;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use vpin::vpx::gameitem::ramp;
use vpin::vpx::gameitem::ramp::RampType;
use vpin::vpx::units::vpu_to_m;

/// Flat ramps wider than this (in vpx units) are treated as ride-on ramp floors, not
/// guide rails, so they do not become 2D wall colliders. Thin flat ramps below it (e.g.
/// North Pole's `MetalGuide001`/`3` inlane rails, width 2) are guides.
const GUIDE_MAX_WIDTH_VPU: f32 = 15.0;

// Chrome wire shading, mirroring the ball's overhead lights so wires and ball catch the
// same highlights (see `ball::BALL_LIGHTS`).
const WIRE_LIGHTS: [Vec2; 2] = [Vec2::new(0.5, 0.7), Vec2::new(-0.5, 0.7)];
const WIRE_LIGHT_ELEVATION: f32 = 0.8;
const WIRE_LIGHT_INTENSITY: f32 = 0.6;
const WIRE_SHININESS: f32 = 60.0;
const WIRE_TINT: Vec3 = Vec3::new(0.95, 0.96, 1.0);

pub(super) fn plugin(app: &mut App) {
    app.add_plugins(Material2dPlugin::<WireMaterial>::default());
}

/// Shader inputs for [`WireMaterial`]; see `shaders/wire.wgsl`.
#[derive(Clone, Copy, ShaderType)]
struct WireUniform {
    /// rgb = reflection tint, a = specular shininess.
    tint: Vec4,
    /// xy = screen-space light direction (y up), z = elevation, w = intensity.
    light0: Vec4,
    light1: Vec4,
}

/// Renders a wire ramp as polished chrome: the fragment fakes a cylinder normal across
/// the ribbon and reflects the same distant environment map the ball uses.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(crate) struct WireMaterial {
    #[uniform(0)]
    uniform: WireUniform,
    #[texture(1)]
    #[sampler(2)]
    env: Handle<Image>,
}

impl WireMaterial {
    fn chrome(env: Handle<Image>) -> Self {
        let light = |dir: Vec2| {
            dir.normalize()
                .extend(WIRE_LIGHT_ELEVATION)
                .extend(WIRE_LIGHT_INTENSITY)
        };
        Self {
            uniform: WireUniform {
                tint: WIRE_TINT.extend(WIRE_SHININESS),
                light0: light(WIRE_LIGHTS[0]),
                light1: light(WIRE_LIGHTS[1]),
            },
            env,
        }
    }
}

impl Material2d for WireMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/wire.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/wire.wgsl".into()
    }
}

/// Whether a ramp is a wire ramp (any wire type), rendered as a chrome ribbon.
fn is_wire(ramp: &ramp::Ramp) -> bool {
    !matches!(ramp.ramp_type, RampType::Flat)
}

#[derive(Component)]
pub struct Ramp {
    /// The vpx ramp name; kept for debugging and tooling.
    #[allow(dead_code)]
    pub name: String,
}

/// Whether a ramp acts as a 2D wall (a thin barrier the ball cannot pass) rather than a
/// surface the ball rides. One-wire ramps are single guide wires; thin flat ramps are
/// rails. Multi-wire habitrails and wide flat floors carry the ball, so they are not
/// walls in this projection.
fn is_guide(ramp: &ramp::Ramp) -> bool {
    match ramp.ramp_type {
        RampType::OneWire => true,
        RampType::Flat => ramp.width_top.max(ramp.width_bottom) <= GUIDE_MAX_WIDTH_VPU,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_ramp(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    wire_materials: &mut ResMut<Assets<WireMaterial>>,
    env: &Handle<Image>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    ramp: &ramp::Ramp,
) {
    // Mesh is only generated for non-degenerate ramps (see ramp_mesh); skip if absent.
    let Some(mesh_handle) = vpx_asset
        .named_meshes
        .get(VpxAsset::ramp_mesh_sub_path(&ramp.name).as_str())
    else {
        return;
    };

    let mut entity = parent.spawn((
        Name::from(format!("Ramp {}", ramp.name)),
        Ramp {
            name: ramp.name.clone(),
        },
        Mesh2d(mesh_handle.clone()),
        vpx_to_bevy_transform,
    ));

    // Wire ramps render as polished chrome reflecting the same environment as the ball;
    // flat ramps use a plain tinted/textured material from their vpx material (base colour
    // tinted, alpha blending when the material opacity is active, like walls).
    if is_wire(ramp) {
        entity.insert(MeshMaterial2d(
            wire_materials.add(WireMaterial::chrome(env.clone())),
        ));
    } else {
        let material = vpx_asset
            .raw
            .gamedata
            .materials
            .iter()
            .flatten()
            .find(|m| m.name == ramp.material);
        let texture = vpx_asset.image(ramp.image.as_str()).cloned();
        let (color, alpha_mode) = if let Some(mat) = material {
            let alpha = if mat.opacity_active { mat.opacity } else { 1.0 };
            let texture_has_alpha = !vpx_asset
                .raw
                .images
                .iter()
                .find(|i| i.name.eq_ignore_ascii_case(ramp.image.as_str()))
                .and_then(|i| i.is_opaque)
                .unwrap_or(true);
            let blend = mat.opacity_active && (texture_has_alpha || alpha < 0.999);
            let color = Srgba {
                alpha,
                ..Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
            };
            (
                color,
                if blend {
                    AlphaMode2d::Blend
                } else {
                    AlphaMode2d::Opaque
                },
            )
        } else {
            (css::SLATE_GRAY, AlphaMode2d::Opaque)
        };
        entity.insert(MeshMaterial2d(materials.add(ColorMaterial {
            color: color.into(),
            alpha_mode,
            texture,
            uv_transform: Affine2::from_scale(Vec2::splat(0.01)),
        })));
    }

    if ramp.is_visible {
        entity.insert(crate::pinball::light::ShadowCaster { scale: 1.0 });
    } else {
        // Invisible ramps are collision guides in vpinball; we don't draw them.
        entity.insert(Visibility::Hidden);
    }

    // Treat a guide ramp as a wall when it sits within the ball's vertical reach: in 2D
    // the ball cannot pass it. North Pole's `MetalGuide002` is a wire arcing over the
    // lane at ~16mm (height_bottom 30 vpu), low enough that the ball strikes it - the
    // same height gate walls use. A solid trimesh (not a thin polyline) is used so the
    // ball does not wedge or tunnel (see the wall collider notes).
    if ramp.is_collidable
        && is_guide(ramp)
        && vpu_to_m(ramp.height_bottom) < BALL_RADIUS_M * 2.0
        && let Some(mesh) = meshes.get(mesh_handle)
    {
        entity.insert((
            RigidBody::Static,
            mesh_collider(mesh),
            Restitution::from(ramp.elasticity),
            Friction::from(ramp.friction),
        ));
    }
}
