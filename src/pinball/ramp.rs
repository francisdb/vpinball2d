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
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::ramp;
use vpin::vpx::gameitem::ramp::RampType;
use vpin::vpx::units::vpu_to_m;

/// Flat ramps wider than this (in vpx units) are treated as ride-on ramp floors, not
/// guide rails, so they do not become 2D wall colliders. Thin flat ramps below it (e.g.
/// North Pole's `MetalGuide001`/`3` inlane rails, width 2) are guides.
const GUIDE_MAX_WIDTH_VPU: f32 = 15.0;

pub(super) fn plugin(_app: &mut App) {}

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

pub(super) fn spawn_ramp(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    ramp: &ramp::Ramp,
    item_index: usize,
) {
    // Mesh is only generated for non-degenerate ramps (see ramp_mesh); skip if absent.
    let Some(mesh_handle) = vpx_asset
        .named_meshes
        .get(VpxAsset::ramp_mesh_sub_path(&ramp.name).as_str())
    else {
        return;
    };

    // Colour/transparency from the ramp material, mirroring how walls resolve their top
    // material (base colour tinted, alpha blending when the material opacity is active).
    let material = vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == ramp.material);
    let texture = vpx_asset.image(ramp.image.as_str()).cloned();
    let texture_has_alpha = !vpx_asset
        .raw
        .images
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(ramp.image.as_str()))
        .and_then(|i| i.is_opaque)
        .unwrap_or(true);
    let (color, alpha_mode) = if let Some(mat) = material {
        let alpha = if mat.opacity_active { mat.opacity } else { 1.0 };
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

    let material = materials.add(ColorMaterial {
        color: color.into(),
        alpha_mode,
        texture: texture.clone(),
        ..default()
    });

    // The ramp draws at its centre height minus its depth bias (see layer.rs):
    // transparent 2D sorting only sees the entity transform, so e.g. the apron score
    // cards at 51 vpu sort above the playfield and below the apron, and an elevated
    // ramp covers lower geometry. The mesh vertices carry per-point offsets relative
    // to this height.
    let transform = Transform::from_xyz(
        vpx_to_bevy_transform.translation.x,
        vpx_to_bevy_transform.translation.y,
        crate::pinball::layer::render_z(
            (ramp.height_bottom + ramp.height_top) * 0.5,
            ramp.depth_bias,
            item_index,
        ),
    );
    let mut entity = parent.spawn((
        Name::from(format!("Ramp {}", ramp.name)),
        Ramp {
            name: ramp.name.clone(),
        },
        Mesh2d(mesh_handle.clone()),
        MeshMaterial2d(material),
        transform,
    ));
    // A ramp lying fully below the playfield (e.g. North Pole's subway return at
    // -62 vpu) is hidden by the playfield itself in vpinball, so it must not drop a
    // shadow onto it either. A ramp whose low end is near the playfield casts its
    // full silhouette; a raised plastics sheet (e.g. 8 Ball's full-table Ramp19)
    // casts only its cut-out art, and only when sheet-sized - raised score cards
    // and the like must not shade the playfield (see ShadowCaster).
    let below_playfield = ramp.height_bottom.max(ramp.height_top) < 0.0;
    let base = ramp.height_bottom.min(ramp.height_top);
    let shadow_caster = if crate::pinball::light::casts_playfield_shadow(base) {
        Some(crate::pinball::light::ShadowCaster {
            scale: 1.0,
            texture: None,
        })
    } else if crate::pinball::light::footprint_fraction(
        ramp.drag_points.iter().map(|d| (d.x, d.y)),
        ramp.width_bottom.max(ramp.width_top) * 0.5,
        &vpx_asset.raw.gamedata,
    ) >= crate::pinball::light::RAISED_MIN_TABLE_FRACTION
    {
        // A raised panel whose mesh is its shape (untextured/opaque) casts a solid
        // silhouette; a sheet with cut-out art in the texture casts the art.
        Some(crate::pinball::light::ShadowCaster {
            scale: 1.0,
            texture: if texture_has_alpha {
                texture.clone()
            } else {
                None
            },
        })
    } else {
        None
    };
    if !ramp.is_visible || below_playfield {
        // Invisible ramps are collision guides in vpinball; we don't draw them, and
        // a below-playfield ramp is hidden by the playfield itself.
        entity.insert(Visibility::Hidden);
    } else if let Some(caster) = shadow_caster {
        entity.insert(caster);
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
