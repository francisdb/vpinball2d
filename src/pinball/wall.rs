use crate::PausableSystems;
use crate::audio::play_sound_at;
use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::rubber::Rubber;
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::math::Vector;
use core::time::Duration;

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
        (
            strip_slingshot_rest_colliders,
            handle_slingshot_collisions,
            animate_slingshot_flash,
        )
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

/// A slingshot's rest rubber sits coincident with (slightly in front of) the slingshot
/// wall. With both collidable the ball bounces off the rubber before reaching the wall, so
/// the kick never fires. Match vpinball's effective behaviour by letting the wall be the
/// single strike+kick surface: strip the collider from each slingshot's rest rubber,
/// leaving it visual only. Runs when rubbers are spawned (`Added`).
fn strip_slingshot_rest_colliders(
    mut commands: Commands,
    animations: Option<Res<SlingshotAnimations>>,
    rubbers: Query<(Entity, &Rubber), Added<Rubber>>,
) {
    let Some(animations) = animations else {
        return;
    };
    for (entity, rubber) in &rubbers {
        if animations.0.iter().any(|a| a.rest == rubber.name) {
            commands
                .entity(entity)
                .remove::<Collider>()
                .remove::<CollisionEventsEnabled>();
        }
    }
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
    /// The slingshot wall's name (matches the vpx Wall name), used to find its animation.
    pub(crate) name: String,
    /// Outward impulse strength (already scaled into this world's units).
    pub(crate) force: f32,
    /// Minimum inbound speed (m/s) before the slingshot fires.
    pub(crate) threshold: f32,
    /// World-space centre of the slingshot, used to derive the outward kick direction.
    pub(crate) center: Vec2,
}

/// Sounds a table plays when a slingshot fires. A random entry is picked and played
/// spatially at the slingshot (so left/right panning comes from its position). A table
/// enables slingshot sounds by inserting this resource.
#[derive(Resource, Default)]
pub struct SlingshotSounds {
    pub hit: Vec<String>,
}

/// How long the flexed slingshot rubber is shown after a hit before reverting to rest.
/// vpinball resets the slingshot animation ~100 ms after firing.
const SLINGSHOT_FLASH: Duration = Duration::from_millis(100);

/// Links a slingshot (by wall name) to the rubbers that visualise it, so firing it can
/// briefly show the flexed rubber. The rubber/wall names are table conventions (e.g.
/// `LeftSlingShot` -> rest `LSling`, flexed `LSling1`), driven by the script in vpinball,
/// so a table enables the animation by inserting this resource.
#[derive(Resource, Default)]
pub struct SlingshotAnimations(pub Vec<SlingshotAnimation>);

pub struct SlingshotAnimation {
    /// Slingshot wall name.
    pub slingshot: String,
    /// Rubber shown at rest.
    pub rest: String,
    /// Rubber shown briefly while the slingshot is fired.
    pub flexed: String,
}

/// Active flex animation on a slingshot: the flexed rubber is shown until the timer ends,
/// then visibility reverts to the rest rubber.
#[derive(Component)]
struct SlingshotFlash {
    rest: Entity,
    flexed: Entity,
    timer: Timer,
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
                    name: wall.name.clone(),
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

/// When a ball hits a slingshot fast enough, kick it back out. The kick direction comes
/// from the slingshot's own deformation: a real slingshot's solenoid bulges the rubber
/// band into a triangle pointing the way it throws the ball, so we kick along
/// `flexed_rubber_centre - rest_rubber_centre`. The magnitude is a constant impulse above
/// the speed threshold (a solenoid fires the same each time), tuned via the scale consts.
/// Falls back to "slingshot centre -> ball" when the table has no rubber animation mapping.
#[allow(clippy::too_many_arguments)]
fn handle_slingshot_collisions(
    mut collision_reader: MessageReader<CollisionStart>,
    slingshot_query: Query<&Slingshot>,
    // `Forces` already holds `&mut LinearVelocity`, so read the velocity through it rather
    // than adding a conflicting `&LinearVelocity` to the same query.
    mut ball_query: Query<(&Transform, Forces), With<Ball>>,
    collisions: Collisions,
    sounds: Option<Res<SlingshotSounds>>,
    table_assets: Option<Res<TableAssets>>,
    assets_vpx: Res<Assets<VpxAsset>>,
    animations: Option<Res<SlingshotAnimations>>,
    rubbers: Query<(Entity, &Rubber)>,
    mut visibility: Query<&mut Visibility>,
    flashing: Query<(), With<SlingshotFlash>>,
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

        // The rest/flexed rubbers (if the table maps them) drive both the kick direction
        // and the flex animation.
        let anim = animations
            .as_ref()
            .and_then(|a| a.0.iter().find(|a| a.slingshot == slingshot.name));
        let rubber_entity =
            |name: &str| rubbers.iter().find(|(_, r)| r.name == name).map(|(e, _)| e);
        let rest_flexed =
            anim.and_then(|a| Some((rubber_entity(&a.rest)?, rubber_entity(&a.flexed)?)));

        // Kick direction: from the rest rubber towards the flexed (extended) one, i.e. the
        // way the band bulges. Fall back to centre -> ball when there is no mapping.
        let ball_pos = ball_transform.translation.truncate();
        let outward = rest_flexed
            .and_then(|(rest, flexed)| {
                let rest_c = rubbers.get(rest).ok()?.1.center;
                let flexed_c = rubbers.get(flexed).ok()?.1.center;
                (flexed_c - rest_c).try_normalize()
            })
            .unwrap_or_else(|| (ball_pos - slingshot.center).normalize_or_zero());

        // Fire on the impact velocity, read from the contact's pre-solve `normal_speed` (avian
        // stores the relative normal velocity at the contact, negative when approaching). Reading
        // the ball's `LinearVelocity` here would be wrong: this `Update` system runs after the
        // solver has already bounced the ball back outward, so a clean head-on hit looks like it
        // is moving away and never fires - only glancing hits keep enough inbound velocity to slip
        // through. The contact velocity is mass-independent and survives the solve, so a
        // straight-on hit reads as the strongest approach. Take the strongest across the points.
        let inbound = collisions
            .get(collision.collider1, collision.collider2)
            .into_iter()
            .flat_map(|pair| pair.manifolds.iter())
            .flat_map(|manifold| manifold.points.iter())
            .map(|point| -point.normal_speed)
            .fold(0.0_f32, f32::max);
        if inbound < slingshot.threshold {
            continue;
        }

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

        // flex animation: briefly show the flexed rubber instead of the rest one
        if let Some((rest, flexed)) = rest_flexed
            && !flashing.contains(slingshot_entity)
        {
            set_visibility(&mut visibility, rest, Visibility::Hidden);
            set_visibility(&mut visibility, flexed, Visibility::Inherited);
            commands.entity(slingshot_entity).insert(SlingshotFlash {
                rest,
                flexed,
                timer: Timer::new(SLINGSHOT_FLASH, TimerMode::Once),
            });
        }
    }
}

/// Revert a slingshot's flex animation once its timer elapses: hide the flexed rubber and
/// show the rest rubber again.
fn animate_slingshot_flash(
    time: Res<Time>,
    mut flashes: Query<(Entity, &mut SlingshotFlash)>,
    mut visibility: Query<&mut Visibility>,
    mut commands: Commands,
) {
    for (entity, mut flash) in &mut flashes {
        if flash.timer.tick(time.delta()).just_finished() {
            set_visibility(&mut visibility, flash.flexed, Visibility::Hidden);
            set_visibility(&mut visibility, flash.rest, Visibility::Inherited);
            commands.entity(entity).remove::<SlingshotFlash>();
        }
    }
}

fn set_visibility(query: &mut Query<&mut Visibility>, entity: Entity, value: Visibility) {
    if let Ok(mut visibility) = query.get_mut(entity) {
        *visibility = value;
    }
}
