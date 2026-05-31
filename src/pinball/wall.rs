use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::math::Vector;

use avian2d::prelude::*;
use bevy::color::palettes::css;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::sprite_render::AlphaMode2d;
use vpin::vpx::gameitem::dragpoint::DragPoint;
use vpin::vpx::gameitem::wall;
use vpin::vpx::units::vpu_to_m;

/// vpinball measures slingshot strength in its own units and scales it by 1/10 internally
/// (`Surface::GetSlingshotStrength`). We additionally scale into this 2D world's impulse
/// units; tuned so a normal hit kicks the ball back into play. TODO calibrate against vpx.
const SLINGSHOT_FORCE_SCALE: f32 = 0.1 * 0.02;
/// Minimum ball speed towards the slingshot face (m/s) for it to fire. vpinball uses the
/// per-surface `slingshot_threshold`; we scale it into m/s. TODO calibrate against vpx.
const SLINGSHOT_THRESHOLD_SCALE: f32 = 0.05;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        handle_slingshot_collisions
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

#[derive(Component)]
pub struct Wall {
    pub name: String,
}

/// A slingshot wall: when a ball hits it fast enough, it kicks the ball back out. Modelled
/// after vpinball's `LineSegSlingshot`, where a wall segment flagged `is_slingshot` reflects
/// the ball and, above `slingshot_threshold`, adds an outward `slingshot_force` impulse.
#[derive(Component)]
pub struct Slingshot {
    /// Outward impulse strength (already scaled into this world's units).
    force: f32,
    /// Minimum inbound speed (m/s) before the slingshot fires.
    threshold: f32,
    /// World-space centre of the slingshot, used to derive the outward kick direction.
    center: Vec2,
}

/// Sounds a table plays when a slingshot fires. A random entry is picked and played
/// spatially at the slingshot (so left/right panning comes from its position). A table
/// enables slingshot sounds by inserting this resource.
#[derive(Resource, Default)]
pub struct SlingshotSounds {
    pub hit: Vec<String>,
}

pub(super) fn spawn_wall(
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    meshes: &ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    wall: &wall::Wall,
) {
    let mesh_handle = vpx_asset
        .named_meshes
        .get(VpxAsset::wall_mesh_sub_path(&wall.name).as_str())
        .unwrap();
    //let color = css::PINK;
    let top_material = vpx_asset
        .raw
        .gamedata
        .materials
        .iter()
        .flatten()
        .find(|m| m.name == wall.top_material);
    let color = if let Some(mat) = top_material {
        Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
    } else {
        css::PINK
    };
    let texture = vpx_asset.named_images.get(wall.image.as_str()).cloned();
    let mut mat = ColorMaterial {
        color: color.into(),
        alpha_mode: AlphaMode2d::Opaque,
        texture,
        // TODO adjust UV scale properly, how doe vpinball do this?
        uv_transform: Affine2::from_scale(Vec2::splat(0.01)),
    };
    if !wall.is_top_bottom_visible && !wall.is_side_visible {
        mat.alpha_mode = AlphaMode2d::Blend;
        mat.color = color.with_alpha(0.5).into();
    }
    let material = materials.add(mat);
    let name_component = Name::from(format!("Wall {}", wall.name));
    let wall_component = Wall {
        name: wall.name.clone(),
    };
    // A wall collides with the ball when its vertical span reaches into the ball's height.
    // VPX wall heights are in vpu, so convert to metres before comparing with the ball size.
    //   - height_bottom below the ball top: not floating above the ball (e.g. raised plastics
    //     at 50 vpu sit at the ball top and stay visual; slingshot guides at 30 vpu collide)
    //   - height_top above the playfield: not sunk below it (e.g. the trigger wire hole)
    if wall.is_collidable
        && vpu_to_m(wall.height_bottom) < BALL_RADIUS_M * 2.0
        && wall.height_top > 0.0
    {
        let mesh = meshes.get(mesh_handle).unwrap();
        let collider = mesh_collider(mesh);
        let mut entity = parent.spawn((
            name_component,
            wall_component,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            vpx_to_bevy_transform,
            RigidBody::Static,
            Restitution::from(wall.elasticity),
            Friction::from(wall.friction),
            collider,
        ));
        // A wall is a slingshot when it has a threshold and a drag point flagged as such
        // (vpinball builds slingshot segments from `is_slingshot` drag points).
        let is_slingshot = wall.slingshot_threshold > 0.0
            && wall
                .drag_points
                .iter()
                .any(|dp| dp.is_slingshot == Some(true));
        if is_slingshot {
            entity.insert((
                Slingshot {
                    force: wall.slingshot_force * SLINGSHOT_FORCE_SCALE,
                    threshold: wall.slingshot_threshold * SLINGSHOT_THRESHOLD_SCALE,
                    center: slingshot_center(&wall.drag_points, vpx_to_bevy_transform),
                },
                CollisionEventsEnabled,
                // The slingshot's visual band is a separate Rubber gameitem; hide the wall
                // mesh so we don't draw the band twice (rest rubber + extended wall sliver).
                Visibility::Hidden,
            ));
        }
    } else {
        parent.spawn((
            name_component,
            wall_component,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            vpx_to_bevy_transform,
        ));
    }
}

/// Create a polyline collider from the 2D mesh vertices
pub(super) fn mesh_collider(mesh: &Mesh) -> Collider {
    let vertices: Vec<Vector> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap()
        .iter()
        .map(|v| Vector::new(v[0], v[1]))
        .collect();
    // we have to duplicate the first vertex at the end to close the loop
    let mut vertices = vertices;
    vertices.push(vertices[0]);
    Collider::polyline(vertices, None)
}

/// World-space centre of a wall's drag points (vpx coords -> bevy, like the mesh).
fn slingshot_center(drag_points: &[DragPoint], transform: Transform) -> Vec2 {
    let sum: Vec2 = drag_points.iter().fold(Vec2::ZERO, |acc, dp| {
        acc + Vec2::new(vpu_to_m(dp.x), -vpu_to_m(dp.y))
    });
    let mean = sum / drag_points.len().max(1) as f32;
    transform.translation.truncate() + mean
}

/// When a ball hits a slingshot fast enough, kick it back out along the outward direction
/// (from the slingshot centre towards the ball). Mirrors vpinball's `LineSegSlingshot`:
/// reflect happens via the static collider; this adds the extra slingshot impulse.
#[allow(clippy::too_many_arguments)]
fn handle_slingshot_collisions(
    mut collision_reader: MessageReader<CollisionStart>,
    slingshot_query: Query<&Slingshot>,
    // `Forces` already holds `&mut LinearVelocity`, so read the velocity through it rather
    // than adding a conflicting `&LinearVelocity` to the same query.
    mut ball_query: Query<(&Transform, Forces), With<Ball>>,
    sounds: Option<Res<SlingshotSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
    mut commands: Commands,
) {
    for collision in collision_reader.read() {
        let (Some(b1), Some(b2)) = (collision.body1, collision.body2) else {
            continue;
        };
        // Identify which body is the slingshot and which is the ball.
        let (slingshot_entity, ball_entity) = if slingshot_query.contains(b1) {
            (b1, b2)
        } else if slingshot_query.contains(b2) {
            (b2, b1)
        } else {
            continue;
        };
        let slingshot = slingshot_query.get(slingshot_entity).unwrap();
        let Ok((ball_transform, mut forces)) = ball_query.get_mut(ball_entity) else {
            continue;
        };

        let ball_pos = ball_transform.translation.truncate();
        let outward = (ball_pos - slingshot.center).normalize_or_zero();
        // Inbound speed towards the slingshot face (positive when moving into it).
        let inbound = -forces.linear_velocity().dot(outward);
        if inbound >= slingshot.threshold {
            forces.apply_linear_impulse(outward * slingshot.force);
            // play the slingshot sound at the slingshot (spatial panning from its position)
            if let (Some(sounds), Some(table_assets)) = (&sounds, &table_assets) {
                play_sound_at(
                    &mut commands,
                    table_assets,
                    &assets_vpx,
                    slingshot_entity,
                    &sounds.hit,
                );
            }
        }
    }
}
