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
use bevy::mesh::Indices;
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
/// Width (vpx units) of the band that visualises a wall's translucent side faces
/// (edge-lit acrylic outlines).
const SIDE_BAND_WIDTH_VPU: f32 = 6.0;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (strip_slingshot_rest_colliders, animate_slingshot_flash)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
    // Read contacts in the physics schedule (per fixed step, right after avian's step) so the
    // slingshot sees the ball's approach speed. Read in `Update` it would be stale by many physics
    // steps whenever the tick rate outpaces the frame rate, and the inbound speed reads as ~0.
    app.add_systems(
        FixedPostUpdate,
        handle_slingshot_collisions
            .after(PhysicsSystems::StepSimulation)
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
    /// The vpx wall name; kept for debugging and tooling even when nothing reads it.
    #[allow(dead_code)]
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
    /// World-space centre of the slingshot, used to orient the kick towards the ball.
    pub(crate) center: Vec2,
    /// Unit normal of the kicking segment (sign arbitrary; oriented towards the ball at hit
    /// time). vpinball applies the slingshot force along this segment normal.
    normal: Vec2,
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
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    wall: &wall::Wall,
    item_index: usize,
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
    let texture = vpx_asset.image(wall.image.as_str()).cloned();
    // vpinball's `has_alpha` = the image is not opaque (see Shader::SetBasic).
    let image_data = vpx_asset
        .raw
        .images
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(wall.image.as_str()));
    let texture_has_alpha = !image_data.and_then(|i| i.is_opaque).unwrap_or(true);
    // Mirror vpinball's surface-top rendering (Shader::SetMaterial): the base colour
    // carries the material opacity as its alpha, and alpha blending is enabled only
    // when the material has opacity active and either the texture has an alpha channel
    // or the opacity is meaningfully below 1. Without this a plastics sheet (a texture
    // with transparent areas around each plastic) renders the gaps as solid base colour
    // instead of letting the playfield show through.
    let (color, alpha_mode) = if let Some(mat) = top_material {
        let alpha = if mat.opacity_active { mat.opacity } else { 1.0 };
        let blend = mat.opacity_active && (texture_has_alpha || alpha < 0.999);
        let color = Srgba {
            alpha,
            ..Srgba::rgb_u8(mat.base_color.r, mat.base_color.g, mat.base_color.b)
        };
        // Without blending, vpinball still alpha-tests a non-opaque texture when the
        // image has a test value set (Shader::SetBasic), discarding pixels with
        // alpha <= value/255. That is how cut-out plastics sheets on a wall top get
        // their holes; map it to AlphaMode2d::Mask.
        let alpha_test = image_data.map(|i| i.alpha_test_value).unwrap_or(-1.0);
        let alpha_mode = if blend {
            AlphaMode2d::Blend
        } else if texture_has_alpha && alpha_test >= 0.0 {
            AlphaMode2d::Mask(alpha_test / 255.0)
        } else {
            AlphaMode2d::Opaque
        };
        (color, alpha_mode)
    } else {
        (css::PINK, AlphaMode2d::Opaque)
    };
    let material = materials.add(ColorMaterial {
        color: color.into(),
        alpha_mode,
        texture: texture.clone(),
        ..default()
    });
    // A wall with neither face visible is a collision-only guide (e.g. the plunger
    // ball-centering wall); it collides but is not drawn.
    let visible = wall.is_top_bottom_visible || wall.is_side_visible;
    // The wall top draws at the wall's centre height (see layer.rs): transparent 2D
    // sorting only sees the entity transform, so the height must live there (not in
    // the mesh vertices) for e.g. an apron at 52 vpu to cover the ball (drawn at its
    // radius) rolling underneath. Walls have no depth bias in vpx.
    let transform = Transform::from_xyz(
        vpx_to_bevy_transform.translation.x,
        vpx_to_bevy_transform.translation.y,
        crate::pinball::layer::render_z(
            (wall.height_bottom + wall.height_top) * 0.5,
            0.0,
            item_index,
        ),
    );
    let name_component = Name::from(format!("Wall {}", wall.name));
    let wall_component = Wall {
        name: wall.name.clone(),
    };
    // vpinball also draws the wall's extruded side faces. Straight from above they
    // have no area, but translucent sides read as a glowing outline around the top
    // (TNA's edge-lit blacklight acrylics, clear plastic protectors); draw them as
    // a thin band along the wall outline, just above the top.
    if wall.is_side_visible
        && let Some(side) = vpx_asset
            .raw
            .gamedata
            .materials
            .iter()
            .flatten()
            .find(|m| m.name == wall.side_material)
        && side.opacity_active
        && side.opacity < 0.999
    {
        let outline: Vec<Vec2> =
            vpin::vpx::mesh::smooth_drag_points_2d(&wall.drag_points, 4.0, true)
                .iter()
                .map(|(x, y)| Vec2::new(vpu_to_m(*x), -vpu_to_m(*y)))
                .collect();
        if outline.len() >= 3 {
            let band = crate::pinball::rubber::rubber_ring_mesh(
                &outline,
                vpu_to_m(SIDE_BAND_WIDTH_VPU) * 0.5,
            );
            let color = Srgba {
                alpha: side.opacity,
                ..Srgba::rgb_u8(side.base_color.r, side.base_color.g, side.base_color.b)
            };
            parent.spawn((
                Name::from(format!("Wall {} edge", wall.name)),
                Mesh2d(meshes.add(band)),
                MeshMaterial2d(materials.add(ColorMaterial {
                    color: color.into(),
                    alpha_mode: AlphaMode2d::Blend,
                    ..default()
                })),
                // Just above the wall top, so the edge glow reads over the fill.
                Transform::from_xyz(
                    vpx_to_bevy_transform.translation.x,
                    vpx_to_bevy_transform.translation.y,
                    transform.translation.z + vpu_to_m(1.0),
                ),
            ));
        }
    }
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
            transform,
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
                    normal: slingshot_normal(&wall.drag_points, vpx_to_bevy_transform),
                },
                CollisionEventsEnabled,
                // The slingshot's visual band is a separate Rubber gameitem; hide the wall
                // mesh so we don't draw the band twice (rest rubber + extended wall sliver).
                Visibility::Hidden,
            ));
        } else if visible {
            // Visible walls standing on the playfield drop a shadow into the light
            // map through the static-shadow render pass.
            entity.insert(crate::pinball::lightmap::casts_shadow_layers());
        } else {
            // Invisible guide wall: collide but don't draw or cast a shadow.
            entity.insert(Visibility::Hidden);
        }
    } else if visible {
        let mut entity = parent.spawn((
            name_component,
            wall_component,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            transform,
        ));
        entity.insert(crate::pinball::lightmap::casts_shadow_layers());
    } else {
        parent.spawn((
            name_component,
            wall_component,
            Mesh2d(mesh_handle.clone()),
            MeshMaterial2d(material),
            transform,
            Visibility::Hidden,
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
    // Build a *solid* trimesh from the wall's own triangulation (the mesh already carries it). A
    // `polyline` collider has no inside, so it collides from both sides: when the wall is thinner
    // than the ball (rails are ~3 mm, Wall78 ~19 mm, vs the 27 mm ball) the ball engulfs the strip
    // and gets conflicting contacts from both edges - it ricochets instead of sliding ("bounces off
    // an unreachable corner"). A solid trimesh makes the ball hit one surface and slide. We use the
    // exact triangulation rather than `convex_decomposition`, whose VHACD voxelises at ~8 mm and so
    // bulges thin rails, catching the ball mid-roll.
    let tris: Vec<[u32; 3]> = match mesh.indices() {
        Some(Indices::U32(idx)) => idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        Some(Indices::U16(idx)) => idx
            .chunks_exact(3)
            .map(|c| [c[0] as u32, c[1] as u32, c[2] as u32])
            .collect(),
        None => Vec::new(),
    };
    if vertices.len() < 3 || tris.is_empty() {
        let mut vertices = vertices;
        vertices.push(vertices[0]);
        return Collider::polyline(vertices, None);
    }
    Collider::trimesh(vertices, tris)
}

/// World-space centre of a wall's drag points (vpx coords -> bevy, like the mesh).
fn slingshot_center(drag_points: &[DragPoint], transform: Transform) -> Vec2 {
    let sum: Vec2 = drag_points.iter().fold(Vec2::ZERO, |acc, dp| {
        acc + Vec2::new(vpu_to_m(dp.x), -vpu_to_m(dp.y))
    });
    let mean = sum / drag_points.len().max(1) as f32;
    transform.translation.truncate() + mean
}

/// Unit normal of the slingshot's kicking segment, the segment that starts at the `is_slingshot`
/// drag point (this is the line vpinball builds the `LineSegSlingshot` from). Perpendicular to
/// that segment; the sign is arbitrary here and is oriented towards the ball at hit time.
fn slingshot_normal(drag_points: &[DragPoint], transform: Transform) -> Vec2 {
    let n = drag_points.len();
    if n < 2 {
        return Vec2::Y;
    }
    let i = drag_points
        .iter()
        .position(|dp| dp.is_slingshot == Some(true))
        .unwrap_or(0);
    let to_world = |dp: &DragPoint| {
        transform.translation.truncate() + Vec2::new(vpu_to_m(dp.x), -vpu_to_m(dp.y))
    };
    let segment = to_world(&drag_points[(i + 1) % n]) - to_world(&drag_points[i]);
    Vec2::new(segment.y, -segment.x).normalize_or_zero()
}

/// When a ball hits a slingshot fast enough, kick it back out. Like vpinball's
/// `LineSegSlingshot`, the kick is along the kicking segment's normal (see [`slingshot_normal`]),
/// oriented out of the face towards the ball. The magnitude is a constant impulse above the speed
/// threshold (a solenoid fires the same each time), tuned via the scale consts. The rest/flexed
/// rubbers, if the table maps them, only drive the brief flex animation.
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

        // The rest/flexed rubbers (if the table maps them) drive the flex animation.
        let anim = animations
            .as_ref()
            .and_then(|a| a.0.iter().find(|a| a.slingshot == slingshot.name));
        let rubber_entity =
            |name: &str| rubbers.iter().find(|(_, r)| r.name == name).map(|(e, _)| e);
        let rest_flexed =
            anim.and_then(|a| Some((rubber_entity(&a.rest)?, rubber_entity(&a.flexed)?)));

        // Kick direction: along the slingshot segment's normal, the way vpinball throws the ball,
        // oriented to point out of the face towards the ball.
        let ball_pos = ball_transform.translation.truncate();
        let outward = if slingshot.normal.dot(ball_pos - slingshot.center) < 0.0 {
            -slingshot.normal
        } else {
            slingshot.normal
        };

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
