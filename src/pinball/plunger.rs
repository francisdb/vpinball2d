//! The plunger: the spring-loaded rod the player pulls and releases to launch the
//! ball up the shooter lane.
//!
//! We model it the way vpinball does: a rod with a flat tip (textured with the
//! table's plunger image when it ships one) and a coil spring behind it that
//! compresses as the rod is pulled back. The rod is a *kinematic* body, so it
//! pushes the ball but is not stopped by the lane walls - it can protrude past the
//! wall that holds the ball, exactly like a real plunger tip reaching the ball.
//!
//! Motion: hold the key to retract the rod against the spring; release to let the
//! spring fire it forward. The launch speed scales with how far it was pulled and
//! the table's `speed_fire`, and the rod overshoots the rest position (compressing
//! the barrel spring) before settling, matching vpinball's behaviour.

use crate::PausableSystems;
use crate::audio::spatial_sound_effect;
use crate::pinball::ball::{BALL_RADIUS_M, Ball};
use crate::pinball::table::TableAssets;
use crate::screens::Screen;
use crate::vpx::VpxAsset;
use avian2d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use vpin::vpx;
use vpin::vpx::units::vpu_to_m;

/// Key that pulls and releases the plunger.
const PLUNGER_KEY: KeyCode = KeyCode::Enter;
/// How far the rod overshoots the rest position when fired before the barrel
/// spring stops it (metres).
const BARREL_OVERSHOOT_M: f32 = 0.012;
/// Peak launch speed (m/s) per unit of the table's `speed_fire` property.
const LAUNCH_SPEED_PER_FIRE: f32 = 0.03;
/// Lower bound on the peak launch speed (m/s) regardless of `speed_fire`.
const MIN_LAUNCH_SPEED: f32 = 1.5;
/// Time (s) to pull the rod fully back when holding the key.
const PULL_TIME_S: f32 = 0.7;
/// Speed (m/s) the rod eases back to rest with after overshooting the barrel.
const RETURN_SPEED: f32 = 0.6;
/// Below this offset the returning rod is considered settled at rest.
const SETTLE_POS_M: f32 = 0.0005;
/// How far (metres) a launching ball rises before normal wall collisions resume.
/// Long enough to clear the walls capping the lane.
const LAUNCH_CLEAR_DIST_M: f32 = 0.1;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        FixedUpdate,
        (plunger_movement, plunger_launch, clear_launching)
            .chain()
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
    app.add_systems(
        Update,
        (plunger_spring_follow, plunger_sound)
            .in_set(PausableSystems)
            .run_if(in_state(Screen::Gameplay)),
    );
}

/// A ball mid-launch. While present, the ball passes through walls (see
/// [`crate::pinball::gate::GateCollisionHooks`]) so it can clear the wall that caps
/// the plunger lane and any rails above it, instead of jamming against them. It is
/// removed once the ball has risen clear of the lane.
#[derive(Component)]
pub(crate) struct Launching {
    /// World y where the launch began; collisions resume once the ball rises clear.
    start_y: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlungerMode {
    /// At rest at the park position, doing nothing.
    Idle,
    /// The key is held; retracting against the spring.
    Pulling,
    /// Released; driving forward at the launch speed until the barrel limit.
    Firing,
    /// Past the barrel limit; easing back to rest.
    Returning,
}

#[derive(Component)]
pub struct Plunger {
    #[allow(dead_code)]
    pub name: String,
    mode: PlungerMode,
    /// World y of the rod-assembly centre at the rest (park) position.
    rest_center_y: f32,
    /// World y of the rod tip (ball end) at rest.
    rest_tip_y: f32,
    /// Half the lane width (metres); the launch only catches a ball within it.
    half_width: f32,
    /// Full retract travel (metres); the rod moves between `-stroke` and rest (0).
    stroke: f32,
    /// Peak forward launch speed (m/s) at a full pull (from the table `speed_fire`).
    launch_speed: f32,
    /// Forward speed captured at release; the rod drives the ball at this until the
    /// barrel limit, so the ball leaves at this speed.
    fire_speed: f32,
    /// Retract speed while pulling (m/s).
    pull_speed: f32,
    /// Current offset from rest along the lane (+ = forward/toward ball), tracked
    /// from the body transform so the spring visual can follow it.
    offset: f32,
}

impl Plunger {
    /// How far the rod is drawn back, 0 (resting) .. 1 (fully pulled).
    pub(crate) fn pulled(&self) -> f32 {
        if self.stroke > 1e-6 {
            (-self.offset / self.stroke).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// The coil spring behind the rod. It is anchored at `bottom_y` and its top
/// follows the rod, so it visually compresses as the rod is pulled back.
#[derive(Component)]
struct PlungerSpring {
    plunger: Entity,
    /// Fixed world y of the spring's anchored (far) end.
    bottom_y: f32,
    /// World y of the spring's top when the rod is at rest.
    rest_top_y: f32,
}

pub(super) fn spawn_plunger(
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<ColorMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    vpx_asset: &VpxAsset,
    vpx_to_bevy_transform: Transform,
    parent: &mut RelatedSpawnerCommands<ChildOf>,
    plunger: &vpx::gameitem::plunger::Plunger,
) {
    let center_x = vpx_to_bevy_transform.translation.x + vpu_to_m(plunger.center.x);
    let width_m = vpu_to_m(plunger.width);
    let length_m = vpu_to_m(plunger.height);
    let stroke_m = vpu_to_m(plunger.stroke);
    let z = vpu_to_m(plunger.height);

    // Rest (park) position of the tip. vpinball rests it `park * stroke` back from the
    // fully-forward limit (`m_pos = frameTop + restPos * frameLen`, frameTop = v.y -
    // stroke), so it sits behind the ball - the ball rests against the lane wall ahead
    // of the tip, not on the tip. (vpx y is flipped into bevy space.)
    let park = plunger.park_position.clamp(0.0, 1.0);
    let rest_tip_y =
        vpx_to_bevy_transform.translation.y - vpu_to_m(plunger.center.y) + stroke_m * (1.0 - park);
    let rest_center_y = rest_tip_y - length_m / 2.0;

    // Main-spring tuning: a fired rod swings from the pulled position toward rest,
    // peaking at ~`launch` m/s. For SHM that peak is sqrt(k) * stroke, so pick k to
    // hit the target launch speed, scaled by the table's fire strength.
    let launch_speed = (plunger.speed_fire * LAUNCH_SPEED_PER_FIRE).max(MIN_LAUNCH_SPEED);

    // The tip/rod/ring use the table's plunger image (an unrolled lathe texture);
    // fall back to a plain metallic rod when the table ships none.
    let rod_material = match vpx_asset.image(plunger.image.as_str()) {
        Some(image) => materials.add(ColorMaterial {
            texture: Some(image.clone()),
            ..default()
        }),
        None => materials.add(ColorMaterial::from(Color::srgb(0.78, 0.79, 0.82))),
    };

    // Collider matching the visible tapered tip (not a full-width box), so the ball
    // rests on the tip the way it looks.
    let collider = Collider::convex_hull(plunger_silhouette(width_m, length_m))
        .unwrap_or_else(|| Collider::rectangle(width_m, length_m));

    // Kinematic rod: pushes the ball but is not blocked by the lane walls, so it can
    // reach past the wall holding the ball. Its visual (textured quad) is a child so
    // it moves with the body.
    let plunger_entity = parent
        .spawn((
            Plunger {
                name: plunger.name.clone(),
                mode: PlungerMode::Idle,
                rest_center_y,
                rest_tip_y,
                half_width: width_m / 2.0,
                stroke: stroke_m,
                launch_speed,
                fire_speed: 0.0,
                pull_speed: stroke_m / PULL_TIME_S,
                offset: 0.0,
            },
            Name::from(format!("Plunger {}", plunger.name)),
            Transform::from_xyz(center_x, rest_center_y, z),
            Visibility::default(),
            RigidBody::Kinematic,
            collider,
            Restitution::new(0.0),
            // The rod fires fast; without continuous detection it tunnels through
            // the ball instead of launching it.
            SweptCcd::default(),
            children![(
                Name::from("Plunger rod"),
                Mesh2d(meshes.add(rod_mesh(width_m, length_m))),
                MeshMaterial2d(rod_material),
                Transform::default(),
            )],
        ))
        .id();

    // Coil spring behind the rod (sibling so it can compress independently). It
    // spans from a fixed anchor up to the rod's near end at rest.
    let spring_width =
        vpu_to_m(plunger.spring_diam) * plunger.width.max(1.0).recip() * width_m + width_m * 0.5;
    let spring_width = spring_width.clamp(width_m * 0.4, width_m);
    let rest_top_y = rest_center_y - length_m / 2.0;
    let bottom_y = rest_top_y - stroke_m * 1.3;
    let rest_len = rest_top_y - bottom_y;
    let spring_texture = images.add(spring_image(plunger.spring_loops.max(4.0) as u32));
    parent.spawn((
        PlungerSpring {
            plunger: plunger_entity,
            bottom_y,
            rest_top_y,
        },
        Name::from("Plunger spring"),
        Mesh2d(meshes.add(Rectangle::new(spring_width, 1.0))),
        MeshMaterial2d(materials.add(ColorMaterial {
            texture: Some(spring_texture),
            ..default()
        })),
        Transform::from_xyz(center_x, (rest_top_y + bottom_y) / 2.0, z)
            .with_scale(Vec3::new(1.0, rest_len, 1.0)),
    ));
}

/// Drive each plunger's kinematic motion from the pull key, integrating against the
/// body's own transform so the physics position stays authoritative.
fn plunger_movement(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut plungers: Query<(&mut Plunger, &Transform, &mut LinearVelocity)>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let held = keyboard_input.pressed(PLUNGER_KEY);

    for (mut plunger, transform, mut velocity) in plungers.iter_mut() {
        // Position state comes from the body itself (kinematic, so unaffected by
        // contacts): offset along the lane, + toward the ball.
        let offset = transform.translation.y - plunger.rest_center_y;
        plunger.offset = offset;

        let new_vel = if held {
            plunger.mode = PlungerMode::Pulling;
            // Retract toward the fully pulled position, then hold there.
            if offset > -plunger.stroke {
                -plunger.pull_speed
            } else {
                0.0
            }
        } else {
            match plunger.mode {
                PlungerMode::Pulling => {
                    // Released: fire forward at a speed set by how far it was pulled.
                    let pulled = (-offset / plunger.stroke).clamp(0.0, 1.0);
                    plunger.fire_speed = (plunger.launch_speed * pulled).max(0.0);
                    plunger.mode = PlungerMode::Firing;
                    plunger.fire_speed
                }
                PlungerMode::Firing => {
                    // Drive forward at the launch speed (so the rod is moving at full
                    // speed when it meets the ball) until it overshoots the barrel.
                    if offset < BARREL_OVERSHOOT_M {
                        plunger.fire_speed
                    } else {
                        plunger.mode = PlungerMode::Returning;
                        -RETURN_SPEED
                    }
                }
                PlungerMode::Returning => {
                    // Ease back to the rest position and stop.
                    if offset <= SETTLE_POS_M {
                        plunger.mode = PlungerMode::Idle;
                        0.0
                    } else {
                        -RETURN_SPEED
                    }
                }
                PlungerMode::Idle => 0.0,
            }
        };

        velocity.x = 0.0;
        velocity.y = new_vel;
    }
}

/// Launch the ball: while firing, a ball resting on the tip is driven up the lane at
/// the rod's fire speed and marked [`Launching`] so it passes the walls capping the
/// lane. The kinematic rod alone can outrun a resting ball at launch speed (the brief
/// contact tunnels), so we transfer its speed directly, the way vpinball imparts the
/// plunger's momentum to the ball.
fn plunger_launch(
    mut commands: Commands,
    plungers: Query<(&Plunger, &Transform)>,
    mut balls: Query<(Entity, &Transform, &mut LinearVelocity), With<Ball>>,
) {
    for (plunger, p_xf) in &plungers {
        if plunger.mode != PlungerMode::Firing || plunger.fire_speed <= 0.0 {
            continue;
        }
        let lane_x = p_xf.translation.x;
        // The loaded ball rests near the tip's rest position regardless of how far the
        // rod is currently retracted, so capture around that fixed point.
        for (ball_entity, b_xf, mut velocity) in balls.iter_mut() {
            let dx = (b_xf.translation.x - lane_x).abs();
            let dy = b_xf.translation.y - plunger.rest_tip_y;
            // A ball resting in the lane ahead of the tip (it rests on the lane wall,
            // up to about a stroke ahead of the parked tip).
            let in_lane = dx < plunger.half_width + BALL_RADIUS_M;
            let ahead_of_tip = (-BALL_RADIUS_M * 2.0..plunger.stroke).contains(&dy);
            if in_lane && ahead_of_tip && velocity.y < plunger.fire_speed {
                velocity.y = plunger.fire_speed;
                commands.entity(ball_entity).insert(Launching {
                    start_y: b_xf.translation.y,
                });
            }
        }
    }
}

/// Restore normal wall collisions once a launching ball has risen clear of the lane
/// (or stopped climbing), so it interacts with the rest of the table again.
fn clear_launching(
    mut commands: Commands,
    balls: Query<(Entity, &Transform, &LinearVelocity, &Launching)>,
) {
    for (entity, transform, velocity, launching) in &balls {
        let risen = transform.translation.y - launching.start_y;
        if velocity.y <= 0.0 || risen > LAUNCH_CLEAR_DIST_M {
            commands.entity(entity).remove::<Launching>();
        }
    }
}

/// Keep each spring anchored at its far end while its top follows the rod, so it
/// compresses as the rod is pulled back.
fn plunger_spring_follow(
    plungers: Query<&Plunger>,
    mut springs: Query<(&PlungerSpring, &mut Transform)>,
) {
    for (spring, mut transform) in springs.iter_mut() {
        let Ok(plunger) = plungers.get(spring.plunger) else {
            continue;
        };
        let top_y = spring.rest_top_y + plunger.offset;
        let len = (top_y - spring.bottom_y).max(0.001);
        transform.translation.y = (top_y + spring.bottom_y) / 2.0;
        transform.scale.y = len;
    }
}

/// The plunger rod silhouette, lathed from vpinball's modern plunger profile
/// (`modernCoords` in `plunger.cpp`): a tapered tip, a ring bulge, then the thinner
/// shaft. Each entry is `(radius, y, tv)` where `radius` is a fraction of the
/// plunger width, `y` runs from the tip (0) back along the rod, and `tv` is the
/// texture coordinate down the plunger image. We render the 2D side silhouette of
/// that lathe (width = 2*radius at each y), so the tip has its real tapered shape
/// rather than a square edge.
const PLUNGER_PROFILE: [(f32, f32, f32); 6] = [
    (0.20, 0.0, 0.00),  // tip front
    (0.30, 3.0, 0.11),  // tip
    (0.35, 5.0, 0.14),  // tip
    (0.35, 23.0, 0.19), // tip base
    (0.45, 23.0, 0.21), // ring (widest)
    (0.25, 24.0, 0.25), // shaft
];

/// World-space y of a profile point at `y` (profile units) for a rod of `length`.
fn profile_y(y: f32, length: f32) -> f32 {
    let y_max = PLUNGER_PROFILE[PLUNGER_PROFILE.len() - 1].1;
    length * 0.5 - (y / y_max) * length
}

/// The rod silhouette outline points (local space), shared by the rod mesh and its
/// collider so the collider matches the visible tapered tip rather than a box.
fn plunger_silhouette(width: f32, length: f32) -> Vec<Vec2> {
    PLUNGER_PROFILE
        .iter()
        .flat_map(|&(r, y, _)| {
            let wy = profile_y(y, length);
            [Vec2::new(-r * width, wy), Vec2::new(r * width, wy)]
        })
        .collect()
}

fn rod_mesh(width: f32, length: f32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    // One left and one right vertex per profile point; the tip (y=0) is at the top.
    for (r, y, tv) in PLUNGER_PROFILE {
        let half_w = r * width;
        let wy = profile_y(y, length);
        positions.push([-half_w, wy, 0.0]);
        uvs.push([0.5 - r, tv]);
        positions.push([half_w, wy, 0.0]);
        uvs.push([0.5 + r, tv]);
    }

    let mut indices: Vec<u32> = Vec::new();
    for i in 0..(PLUNGER_PROFILE.len() as u32 - 1) {
        let (l0, r0, l1, r1) = (i * 2, i * 2 + 1, i * 2 + 2, i * 2 + 3);
        indices.extend_from_slice(&[l0, r0, r1, l0, r1, l1]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// A vertical coil texture: bright bands (one per loop) shaded like a metal wire,
/// on a transparent background so only the coil shows.
fn spring_image(loops: u32) -> Image {
    const W: u32 = 8;
    let loops = loops.clamp(4, 48);
    let rows_per_loop = 6u32;
    let h = loops * rows_per_loop;
    let mut data = Vec::with_capacity((W * h * 4) as usize);
    for y in 0..h {
        // Cylinder shading across each loop: brightest in the middle of the wire.
        let phase = (y % rows_per_loop) as f32 / rows_per_loop as f32; // 0..1
        let wire = (phase * std::f32::consts::PI).sin(); // 0 at gaps, 1 mid-wire
        let lit = 0.25 + 0.75 * wire;
        let v = (lit * 220.0) as u8;
        let alpha = if wire > 0.15 { 255 } else { 0 };
        for _ in 0..W {
            data.extend_from_slice(&[v, v, (v as u16 + 12).min(255) as u8, alpha]);
        }
    }
    Image::new(
        Extent3d {
            width: W,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

fn plunger_sound(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    table_assets: Res<TableAssets>,
    assets_vpx: Res<Assets<VpxAsset>>,
    plunger_query: Query<Entity, With<Plunger>>,
) {
    if keyboard_input.just_pressed(PLUNGER_KEY) {
        let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
        // TODO there is also a slow variant for tna
        let pull_sound = vpx_asset
            .named_sounds
            .get("plungerpull")
            .or(vpx_asset.named_sounds.get("fx_plungerpull"))
            .or(vpx_asset.named_sounds.get("SY_TNA_REV02_Plunger_Pull"));
        if let Some(sound) = pull_sound {
            for plunger_entity in plunger_query.iter() {
                commands
                    .entity(plunger_entity)
                    .with_child(spatial_sound_effect(sound.clone()));
            }
        } else {
            warn!("Plunger pull sound not found in VPX asset");
        }
    }
    if keyboard_input.just_released(PLUNGER_KEY) {
        let vpx_asset = assets_vpx.get(&table_assets.vpx).unwrap();
        // TODO the jpsalas tables have a different sound for a release without the ball
        let release_sound = vpx_asset
            .named_sounds
            .get("plunger")
            .or(vpx_asset.named_sounds.get("fx_plunger"))
            .or(vpx_asset
                .named_sounds
                .get("SY_TNA_REV02_Plunger_Release_Ball_1"));
        if let Some(sound) = release_sound {
            for plunger_entity in plunger_query.iter() {
                commands
                    .entity(plunger_entity)
                    .with_child(spatial_sound_effect(sound.clone()));
            }
        } else {
            warn!("Plunger release sound not found in VPX asset");
        }
    }
}
