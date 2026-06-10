//! Flippers, modelled after Visual Pinball.
//!
//! Researched against the upstream Visual Pinball sources (`src/physics/hitflipper.cpp`,
//! `src/parts/flipper.cpp`) and the `exampleTable.vpx` flipper data:
//!
//!   - A flipper is a rod pivoting at its `center`. It rests at `start_angle` and the
//!     solenoid rotates it to `end_angle`; the angle is clamped to the
//!     `[min(start, end), max(start, end)]` range. VPX angles are in degrees with 0
//!     pointing up and positive angles going clockwise.
//!   - VPX has no left/right notion in the geometry. The swing sense comes from a single
//!     flag `m_direction = (end_angle >= start_angle)`: a right-hand flipper increases
//!     its angle towards the end position, a left-hand flipper decreases it. For a
//!     standard table this lines up with the left/right flipper buttons.
//!   - The coil applies a strong torque towards `end_angle` while the button is held; on
//!     release a weaker spring torque (coil strength * return ratio) pulls it back to
//!     `start_angle`. Near the end of stroke VPX also damps the torque (the "EOS" hold
//!     coil), which we do not model yet.
//!   - The example table is mirror-symmetric: LeftFlipper `120.5 deg -> 70 deg`,
//!     RightFlipper `-120.5 deg -> -70 deg`, centres at x=278 and x=596 vpu.
//!
//! We map VPX angles into bevy (0 points +x, positive counter-clockwise) and drive the
//! bat with a `RevoluteJoint` plus a `ConstantTorque`. The right flipper is the mirror of
//! the left: it pivots on the opposite end of the bat with its body angle turned half a
//! turn, which keeps the joint's relative rotation within (-PI, PI] - the range avian's
//! angle limits compare against (`rotation_difference` comes from `Rotation::angle_between`).

use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use core::f32::consts::{PI, TAU};
use vpin::vpx;
use vpin::vpx::gameitem::GameItemEnum;
use vpin::vpx::gameitem::primitive::Primitive;
use vpin::vpx::units::vpu_to_m;

/// A flipper bat is a textured primitive placed on the flipper pivot. Within this distance
/// (vpx units) a visible textured primitive is a bat candidate. Bats sit essentially on
/// the pivot (0-3 vpu in practice); keep this below the distance of fastening screws and
/// posts around the flipper (~33 vpu upwards).
const FLIPPER_BAT_MAX_DIST_VPU: f32 = 10.0;

/// The textured primitive that is a flipper's bat (its top art, often with text), if any,
/// with its projected top-down mesh. Modern tables model the bat as a primitive on the
/// pivot; we render it rotating with the flipper instead of as a static projection.
///
/// Tables often stack a flat shadow primitive on the same pivot (e.g. A-Go-Go's
/// `priFlipperShadow*` under `priLLFlip`); the bat is the raised one, so among the
/// candidates pick the one whose projected mesh centre is highest.
fn flipper_bat_primitive<'a>(
    vpx_asset: &'a VpxAsset,
    flipper: &vpx::gameitem::flipper::Flipper,
) -> Option<(&'a Primitive, Handle<Mesh>)> {
    let c = &flipper.center;
    let dist2 = |p: &Primitive| (p.position.x - c.x).powi(2) + (p.position.y - c.y).powi(2);
    vpx_asset
        .raw
        .gameitems
        .iter()
        .filter_map(|it| match it {
            GameItemEnum::Primitive(p) if p.is_visible && !p.image.is_empty() => Some(p),
            _ => None,
        })
        // Baked shadow primitives on the pivot are discarded (we generate flipper
        // shadows), so they are never the bat either.
        .filter(|p| !crate::pinball::primitive::is_table_shadow(p))
        .filter(|p| dist2(p) < FLIPPER_BAT_MAX_DIST_VPU.powi(2))
        .filter_map(|p| {
            // Only primitives with a projected mesh can be drawn as the bat.
            let path = VpxAsset::primitive_mesh_sub_path(&p.name);
            let mesh = vpx_asset.named_meshes.get(path.as_str())?;
            let center_z = vpx_asset
                .named_mesh_centers
                .get(path.as_str())
                .copied()
                .unwrap_or(0.0);
            Some((p, mesh.clone(), center_z))
        })
        .max_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(p, mesh, _)| (p, mesh))
}

/// Whether a primitive is a flipper bat, so the general primitive renderer can skip it
/// (the flipper renders it rotating instead). Only the exact primitive chosen as a bat is
/// skipped; co-located decor like a printed flipper shadow still renders statically.
pub(crate) fn is_flipper_bat(vpx_asset: &VpxAsset, primitive: &Primitive) -> bool {
    if !primitive.is_visible || primitive.image.is_empty() {
        return false;
    }
    vpx_asset.raw.gameitems.iter().any(|it| {
        matches!(it, GameItemEnum::Flipper(f)
            if flipper_bat_primitive(vpx_asset, f)
                .is_some_and(|(bat, _)| std::ptr::eq(bat, primitive)))
    })
}

/// Torque the solenoid applies while the flipper button is held.
/// TODO Most flippers also reduce the torque when the flipper is fully extended to avoid burning out the coil.
///   In Visual Pinball this is the "EOS" (end-of-stroke) torque damping near the end angle.
const FLIPPER_ENABLED_TORQUE: f32 = 1.5;
/// Weaker torque from the return spring while the button is released.
/// Visual Pinball models the return as the coil strength scaled by the flipper's
/// return ratio (see `FlipperMoverObject::UpdateVelocities` in hitflipper.cpp).
const FLIPPER_RETURN_TORQUE: f32 = 0.5;

/// Number of segments used to sample each circular arc of the flipper outline.
const FLIPPER_ARC_SEGMENTS: usize = 16;

#[derive(Component)]
pub(crate) struct Flipper {
    #[allow(dead_code)]
    pub name: String,
    /// Body angle (rad) the flipper rests at when released (Visual Pinball start angle).
    pub(crate) rest_angle: f32,
    /// Body angle (rad) the flipper swings to while energised (Visual Pinball end angle).
    pub(crate) active_angle: f32,
    /// Whether the flipper button was held last frame, for sound edge detection.
    pub(crate) pressed: bool,
}

/// Sounds a table plays when a flipper energises (`up`) or returns (`down`). A random
/// entry is picked. A table enables flipper sounds by inserting this resource.
#[derive(Resource, Default)]
pub struct FlipperSounds {
    pub up: Vec<String>,
    pub down: Vec<String>,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        flipper_movement
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

pub(super) fn spawn_flipper(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    flipper: &vpx::gameitem::flipper::Flipper,
    vpx_asset: &VpxAsset,
) {
    let vpx_materials = vpx_asset.raw.gamedata.materials.as_deref().unwrap_or(&[]);
    // Visual Pinball rests the flipper at `start_angle` and the solenoid rotates it to
    // `end_angle`. In vpinball an angle is 0 when the flipper points up and positive
    // angles go clockwise; in bevy 0 points right (+x) and positive angles go
    // counter-clockwise, so the tip direction for each position converts as:
    let rest_tip_dir = (90.0 - flipper.start_angle).to_radians();
    let active_tip_dir = (90.0 - flipper.end_angle).to_radians();

    // Visual Pinball's `m_direction = (end_angle >= start_angle)`: a right-hand flipper
    // increases its angle towards the end position, a left-hand flipper decreases it.
    let right_hand = flipper.end_angle >= flipper.start_angle;

    // Flipper geometry from the VPX radii: a base circle at the pivot and a smaller end
    // circle `flipper_radius_max` away, joined by their outer tangent lines. The rubber
    // band is the outer outline (the ball contact surface); the bat is the same outline
    // inset by the rubber thickness. Both share the pivot, so the bat rotates with it.
    let length = vpu_to_m(flipper.flipper_radius_max);
    let base_radius = vpu_to_m(flipper.base_radius);
    let end_radius = vpu_to_m(flipper.end_radius);
    let rubber_thickness = vpu_to_m(
        flipper
            .rubber_thickness
            .unwrap_or(flipper.rubber_thickness_int as f32),
    );

    // The bat extends towards the tip along the body's +x axis; a right-hand flipper is the
    // mirror image (tip towards -x). Mirroring keeps the joint's relative rotation within
    // (-PI, PI], which is what avian's angle limits compare against.
    let body_turn = if right_hand { PI } else { 0.0 };
    let rubber_outline = flipper_outline(base_radius, end_radius, length, right_hand);
    let bat_outline = flipper_outline(
        base_radius - rubber_thickness,
        end_radius - rubber_thickness,
        length,
        right_hand,
    );

    let rest_angle = normalize_angle(rest_tip_dir - body_turn);
    let active_angle = normalize_angle(active_tip_dir - body_turn);
    let (min_angle, max_angle) = (rest_angle.min(active_angle), rest_angle.max(active_angle));

    // World position of the flipper pivot (its centre in the table).
    let anchor_pos = Vec2::new(
        vpx_to_bevy_transform.translation.x + vpu_to_m(flipper.center.x),
        vpx_to_bevy_transform.translation.y - vpu_to_m(flipper.center.y),
    );

    // The anchor is the static body the joint pivots around; it has no visual.
    let anchor = parent
        .spawn((
            Name::from(format!("Flipper {} Anchor", flipper.name)),
            RigidBody::Static,
            Transform::from_xyz(anchor_pos.x, anchor_pos.y, 0.0),
        ))
        .id();

    let rubber_collider = Collider::convex_hull(rubber_outline.clone())
        .expect("flipper rubber outline should form a valid convex hull");
    // Resolve the rubber and bat colours from the table materials, falling back to the
    // usual red rubber / off-white bat when the named material is missing.
    let rubber_color = material_color(vpx_materials, &flipper.rubber_material, css::RED.into());
    let bat_color = material_color(vpx_materials, &flipper.material, css::ANTIQUE_WHITE.into());

    let rubber_mesh = meshes.add(convex_mesh(&rubber_outline));
    let rubber_material = materials.add(ColorMaterial::from(rubber_color));

    // The bat sits just above the rubber and moves with it. If the table models the bat as
    // a textured primitive on the pivot (e.g. North Pole's lettered bats), draw that art
    // rotating with the flipper; otherwise draw the flat material-coloured bat shape.
    let bat_primitive = flipper_bat_primitive(vpx_asset, flipper);
    let bat_child = if let Some((prim, mesh)) = bat_primitive {
        let material = materials.add(ColorMaterial {
            color: Color::WHITE,
            alpha_mode: AlphaMode2d::Blend,
            texture: vpx_asset.image(prim.image.as_str()).cloned(),
            ..default()
        });
        // The primitive mesh is baked in table space (before the table-centre offset). Place
        // it in the flipper's local frame so it tracks the pivot rotation: undo the rest
        // rotation and the offset between the table origin and the pivot.
        let offset = vpx_to_bevy_transform.translation.truncate() - anchor_pos;
        let local_xy = Mat2::from_angle(-rest_angle) * offset;
        (
            Name::from(format!("Flipper {} Bat", flipper.name)),
            Mesh2d(mesh),
            MeshMaterial2d(material),
            Transform {
                translation: local_xy.extend(0.02),
                rotation: Quat::from_rotation_z(-rest_angle),
                scale: Vec3::ONE,
            },
        )
    } else {
        let bat_mesh = meshes.add(convex_mesh(&bat_outline));
        let bat_material = materials.add(ColorMaterial::from(bat_color));
        (
            Name::from(format!("Flipper {} Bat", flipper.name)),
            Mesh2d(bat_mesh),
            MeshMaterial2d(bat_material),
            Transform::from_xyz(0.0, 0.0, 0.01),
        )
    };

    // Generated drop shadow that swings with the flipper (tables bake these as
    // script-rotated shadow primitives, which we discard; see primitive.rs).
    const FLIPPER_BODY_Z: f32 = 0.1;
    let shadow_child =
        crate::pinball::light::attached_shadow(materials, rubber_mesh.clone(), FLIPPER_BODY_Z, 1.2);

    let flipper_entity = parent
        .spawn((
            Flipper {
                name: flipper.name.clone(),
                rest_angle,
                active_angle,
                pressed: false,
            },
            Name::from(format!("Flipper {}", flipper.name)),
            // the rubber band is the outer shape and the ball contact surface
            Mesh2d(rubber_mesh),
            MeshMaterial2d(rubber_material),
            RigidBody::Dynamic,
            rubber_collider,
            //SleepingDisabled,
            Mass::from(1.0),
            // the rubber makes the flipper bouncy
            Restitution::from(0.4),
            // start at the pivot so the body never overlaps the ball at the world origin;
            // z above the playfield (0.0) so the rubber is not hidden by it
            Transform::from_xyz(anchor_pos.x, anchor_pos.y, FLIPPER_BODY_Z),
        ))
        .with_child(bat_child)
        .with_child(shadow_child)
        .id();

    parent.spawn((
        Name::from(format!("Flipper {} Joint", flipper.name)),
        RevoluteJoint::new(anchor, flipper_entity)
            .with_local_anchor1(Vec2::ZERO)
            // the flipper's base circle is centred on the body origin, which is the pivot
            .with_local_anchor2(Vec2::ZERO)
            .with_angle_limits(min_angle, max_angle),
    ));
}

/// Wrap an angle into the (-PI, PI] range expected by the revolute joint limits.
fn normalize_angle(angle: f32) -> f32 {
    let wrapped = angle.rem_euclid(TAU);
    if wrapped > PI { wrapped - TAU } else { wrapped }
}

/// Outline of a flipper as a convex polygon, with the base circle centred on the pivot
/// (origin) and the end circle `length` away along +x (or -x when `mirror` is set, for a
/// right-hand flipper). The two circles are joined by their outer tangent lines, matching
/// Visual Pinball's flipper footprint.
fn flipper_outline(base_radius: f32, end_radius: f32, length: f32, mirror: bool) -> Vec<Vec2> {
    // Angle of the outer tangent's normal from the +x axis (VPX's "fix angle").
    let psi = ((base_radius - end_radius) / length)
        .clamp(-1.0, 1.0)
        .acos();
    let mut points = Vec::with_capacity(2 * (FLIPPER_ARC_SEGMENTS + 1));
    // Base major arc: from one tangent point around the back to the other.
    for i in 0..=FLIPPER_ARC_SEGMENTS {
        let t = psi + (TAU - 2.0 * psi) * (i as f32 / FLIPPER_ARC_SEGMENTS as f32);
        points.push(Vec2::new(base_radius * t.cos(), base_radius * t.sin()));
    }
    // End minor arc: around the tip.
    for i in 0..=FLIPPER_ARC_SEGMENTS {
        let t = -psi + (2.0 * psi) * (i as f32 / FLIPPER_ARC_SEGMENTS as f32);
        points.push(Vec2::new(
            length + end_radius * t.cos(),
            end_radius * t.sin(),
        ));
    }
    if mirror {
        // Mirror across x and reverse to keep the winding counter-clockwise.
        for p in &mut points {
            p.x = -p.x;
        }
        points.reverse();
    }
    points
}

/// Triangulate a convex, counter-clockwise polygon into a bevy mesh as a triangle fan.
fn convex_mesh(points: &[Vec2]) -> Mesh {
    let positions: Vec<[f32; 3]> = points.iter().map(|p| [p.x, p.y, 0.0]).collect();
    let normals = vec![[0.0, 0.0, 1.0]; points.len()];
    let uvs = vec![[0.0, 0.0]; points.len()];
    let mut indices = Vec::new();
    for i in 1..points.len() as u32 - 1 {
        indices.extend_from_slice(&[0, i, i + 1]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Look up a VPX material by name and return its base colour, or `fallback` when the name
/// is empty or the material is missing from the table.
fn material_color(materials: &[vpx::material::Material], name: &str, fallback: Color) -> Color {
    if name.is_empty() {
        return fallback;
    }
    materials
        .iter()
        .find(|m| m.name == name)
        .map(|m| {
            let c = m.base_color;
            Srgba::rgb_u8(c.r, c.g, c.b).into()
        })
        .unwrap_or_else(|| {
            warn!("Flipper material '{name}' not found, using default color");
            fallback
        })
}

fn flipper_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut flippers: Query<(Entity, &mut Flipper)>,
    sounds: Option<Res<FlipperSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
    mut commands: Commands,
) {
    for (entity, mut flipper) in &mut flippers {
        // The solenoid drives towards the active angle, so the sign of the swing tells us
        // which way the flipper turns: a counter-clockwise (positive) swing is a left-hand
        // flipper, a clockwise (negative) one is right-hand. Visual Pinball has no left/right
        // flipper concept of its own - the table script binds each named flipper to
        // LeftFlipperKey / RightFlipperKey - so we map it to the matching button here.
        let towards_active = (flipper.active_angle - flipper.rest_angle).signum();
        let pressed = if towards_active > 0.0 {
            keyboard_input.pressed(KeyCode::ArrowLeft) || keyboard_input.pressed(KeyCode::ShiftLeft)
        } else {
            keyboard_input.pressed(KeyCode::ArrowRight)
                || keyboard_input.pressed(KeyCode::ShiftRight)
        };

        // While held, drive towards the active angle; when released the return spring pulls
        // back to rest with a weaker torque. Gravity alone is not enough to hold the flipper
        // down, so we always apply a torque towards one of the two limits.
        let torque = if pressed {
            towards_active * FLIPPER_ENABLED_TORQUE
        } else {
            -towards_active * FLIPPER_RETURN_TORQUE
        };
        commands.entity(entity).insert(ConstantTorque(torque));

        // Play the up/down sound on a button-state edge.
        if pressed != flipper.pressed
            && let (Some(sounds), Some(table_assets)) = (&sounds, &table_assets)
        {
            let names = if pressed { &sounds.up } else { &sounds.down };
            play_sound_at(&mut commands, table_assets, &assets_vpx, entity, names);
        }
        flipper.pressed = pressed;
    }
}
