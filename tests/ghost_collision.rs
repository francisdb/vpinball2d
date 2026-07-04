//! Headless, tick-driven physics tests for the "ball bounces off a wall it does not touch" bug and
//! its fix, at our real (tiny) scale.
//!
//! The bug is avian's speculative contacts (avian issue #990, only partially fixed by avian PR
//! #996): a fast ball sliding along a *subdivided* collider (trimesh walls; every curved guide is
//! one) gets contacts against the segment-boundary vertices it merely skims past. The contact
//! normal at such a vertex is tilted, so the tangential slide reads as a strong approach; the
//! solver brakes it and restitution fires, kicking the ball off a surface it never left. The
//! kick grows with speed.
//!
//! The fix is two-part, and both parts are mirrored here (tests cannot import from the game
//! since the crate is a binary):
//! - Swept-path pruning (`pinball::gate::GateCollisionHooks::prune_phantom_contacts`): a purely
//!   speculative (separated) ball/static pair is kept only if casting the ball's shape along its
//!   velocity for one physics step actually reaches the other collider. A phantom vertex skim
//!   misses the cast; a genuine approach (needed for restitution and slingshot inbound-speed
//!   reads) hits it.
//! - A high physics tick rate with few substeps (see main.rs): each narrow-phase frame then
//!   spans only a few millimetres of travel, so a fast ball converging on a subdivided curve
//!   meets its segments a few at a time instead of a frame-long fan of speculative vertex
//!   manifolds that brake it dead (swept CCD arms a velocity-sized reach per pair).
//!
//! Naive alternatives fail and stay documented here: bounding the speculative margin or dropping
//! separated contacts kills restitution (avian derives it from the velocity-clamped normal speed);
//! zeroing manifold restitution on separated contacts kills slow-ball bounces the same way.

use avian2d::math::Vector;
use avian2d::parry::math::Pose;
use avian2d::parry::query::{ShapeCastOptions, cast_shapes};
use avian2d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

const STEP: f64 = 1.0 / 60.0;
const BALL_RADIUS: f32 = 0.0135;
/// The ball slides this far off the wall face; it never touches it.
const SKIM_GAP: f32 = 0.0002;

#[derive(Component)]
struct TestBall;

/// Whether the swept-path pruning is active (the fix under test).
#[derive(Resource, Clone, Copy)]
struct PruneSpeculative(bool);

/// Same pruning as `GateCollisionHooks::prune_phantom_contacts` in the game: shape-cast
/// the ball along its velocity for one physics step; a fully-separated pair whose cast
/// misses is a pass-by and is dropped, a hit or a touching point keeps the pair whole.
#[derive(SystemParam)]
struct SweptPathHooks<'w, 's> {
    enabled: Res<'w, PruneSpeculative>,
    balls: Query<
        'w,
        's,
        (
            &'static Position,
            &'static LinearVelocity,
            &'static Collider,
        ),
        With<TestBall>,
    >,
    others:
        Query<'w, 's, (&'static Position, &'static Rotation, &'static Collider), Without<TestBall>>,
    time: Res<'w, Time>,
}

impl CollisionHooks for SweptPathHooks<'_, '_> {
    fn modify_contacts(&self, contacts: &mut ContactPair, _commands: &mut Commands) -> bool {
        if !self.enabled.0 {
            return true;
        }
        let (ball_entity, other_entity) = if self.balls.contains(contacts.collider1) {
            (contacts.collider1, contacts.collider2)
        } else if self.balls.contains(contacts.collider2) {
            (contacts.collider2, contacts.collider1)
        } else {
            return true;
        };
        let Ok((ball_pos, ball_vel, ball_col)) = self.balls.get(ball_entity) else {
            return true;
        };
        let Ok((other_pos, other_rot, other_col)) = self.others.get(other_entity) else {
            return true;
        };
        let touching = contacts
            .manifolds
            .iter()
            .flat_map(|m| m.points.iter())
            .any(|p| p.penetration >= 0.0);
        let ball_iso = Pose::new(ball_pos.0, 0.0);
        let other_iso = Pose::new(other_pos.0, other_rot.as_radians());
        let hit = cast_shapes(
            &ball_iso,
            ball_vel.0,
            ball_col.shape_scaled().as_ref(),
            &other_iso,
            Vector::ZERO,
            other_col.shape_scaled().as_ref(),
            ShapeCastOptions {
                max_time_of_impact: self.time.delta_secs(),
                target_distance: 0.0,
                stop_at_penetration: true,
                compute_impact_geometry_on_penetration: true,
            },
        )
        .ok()
        .flatten();
        touching || hit.is_some()
    }
}

fn base_app(prune: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::transform::TransformPlugin,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::mesh::MeshPlugin,
        bevy::diagnostic::DiagnosticsPlugin,
    ));
    app.add_plugins(
        PhysicsPlugins::default()
            .with_length_unit(0.1)
            .with_collision_hooks::<SweptPathHooks>(),
    );
    app.insert_resource(PruneSpeculative(prune));
    app.finish();
    app.cleanup();
    app.insert_resource(Gravity(Vector::NEG_Y * 9.81 * 0.12192));
    // Mirror the game's physics rate (see main.rs): high tick rate, few substeps.
    app.insert_resource(SubstepCount(8));
    app.insert_resource(Time::<Fixed>::from_hz(360.0));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        STEP,
    )));
    app
}

fn spawn_ball(app: &mut App, pos: Vec2, vel: Vec2) -> Entity {
    app.world_mut()
        .spawn((
            TestBall,
            RigidBody::Dynamic,
            Collider::circle(BALL_RADIUS),
            Restitution::new(1.0).with_combine_rule(CoefficientCombine::Min),
            Friction::new(1.0).with_combine_rule(CoefficientCombine::Min),
            SweptCcd::default(),
            ActiveCollisionHooks::MODIFY_CONTACTS,
            Transform::from_xyz(pos.x, pos.y, 0.0),
            LinearVelocity(Vector::new(vel.x, vel.y)),
        ))
        .id()
}

/// SKIM: a fast ball slides straight down a hair off a subdivided trimesh wall face (like the
/// game's wall colliders; segment-boundary vertices sit on the face). Returns the max sideways
/// |vx| it picks up; any is a phantom kick off geometry it never touched.
fn trimesh_skim_kick(speed: f32, prune: bool) -> f32 {
    let mut app = base_app(prune);
    // Wall: front face at x = 0 from y = 0 down to -len, back at x = 0.05, the face
    // subdivided into 0.05 m spans (a quad strip, two triangles each), bouncy like a rubber.
    let len = 2.0 + speed * 0.5;
    let segments = (len / 0.05).ceil() as u32;
    let seg_h = len / segments as f32;
    let mut vertices: Vec<Vector> = Vec::new();
    for i in 0..=segments {
        let y = -(i as f32) * seg_h;
        vertices.push(Vector::new(0.0, y));
        vertices.push(Vector::new(0.05, y));
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for i in 0..segments {
        let (f0, b0, f1, b1) = (2 * i, 2 * i + 1, 2 * (i + 1), 2 * (i + 1) + 1);
        tris.push([f0, b0, f1]);
        tris.push([f1, b0, b1]);
    }
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::trimesh(vertices, tris),
        Transform::IDENTITY,
        Restitution::new(0.9),
        Friction::new(0.0),
    ));
    let ball = spawn_ball(
        &mut app,
        Vec2::new(-BALL_RADIUS - SKIM_GAP, -0.05),
        Vec2::new(0.0, -speed),
    );
    let mut max_vx = 0.0f32;
    for _ in 0..4000 {
        app.update();
        let v = app.world().get::<LinearVelocity>(ball).unwrap().0;
        max_vx = max_vx.max(v.x.abs());
        if app.world().get::<Transform>(ball).unwrap().translation.y < -len + 0.1 {
            break;
        }
    }
    println!("skim speed={speed} prune={prune}: max |vx| = {max_vx}");
    max_vx
}

/// ARCH: the ball rides up a wall face into a quarter-circle curve bending left (the top of
/// a shooter lane, like North Pole's Wall002). Both the face and the arc are one subdivided
/// trimesh. A converging fast ball meets a fan of speculative vertex manifolds spread over
/// its whole frame of travel (swept CCD arms a velocity-sized reach) and stalls instead of
/// deflecting. Returns the leftmost x reached: rounding the curve means well past the arc.
fn arch_min_x(speed: f32, prune: bool) -> f32 {
    let mut app = base_app(prune);
    const THICK: f32 = 0.03;
    const ARC_R: f32 = 0.15;
    // Face: x = 0 from y = -1.3 up to 0, then a quarter arc to (-ARC_R, ARC_R).
    let mut face: Vec<Vec2> = Vec::new();
    let mut outward: Vec<Vec2> = Vec::new();
    let mut y = -1.3_f32;
    while y < 0.0 {
        face.push(Vec2::new(0.0, y));
        outward.push(Vec2::X);
        y += 0.05;
    }
    for i in 0..=18 {
        let a = (i as f32) * 5.0_f32.to_radians();
        let dir = Vec2::new(a.cos(), a.sin());
        face.push(Vec2::new(-ARC_R, 0.0) + ARC_R * dir);
        outward.push(dir);
    }
    let mut vertices: Vec<Vector> = Vec::new();
    for (p, n) in face.iter().zip(&outward) {
        vertices.push(Vector::new(p.x, p.y));
        let b = *p + THICK * *n;
        vertices.push(Vector::new(b.x, b.y));
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for i in 0..(face.len() as u32 - 1) {
        let (f0, b0, f1, b1) = (2 * i, 2 * i + 1, 2 * (i + 1), 2 * (i + 1) + 1);
        tris.push([f0, b0, f1]);
        tris.push([f1, b0, b1]);
    }
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::trimesh(vertices, tris),
        Transform::IDENTITY,
        Restitution::new(0.3),
        Friction::new(0.0),
    ));
    let ball = spawn_ball(
        &mut app,
        Vec2::new(-BALL_RADIUS - SKIM_GAP, -1.25),
        Vec2::new(0.0, speed),
    );
    let mut min_x = 1.0f32;
    for i in 0..600 {
        app.update();
        let p = app.world().get::<Transform>(ball).unwrap().translation;
        let v = app.world().get::<LinearVelocity>(ball).unwrap().0;
        if std::env::var("ARCH_TRACE").is_ok() && (i % 5 == 0 || p.y > -0.1) {
            println!(
                "  i={i:3} pos=({:+.3},{:+.3}) v=({:+.2},{:+.2}) speed={:.2}",
                p.x,
                p.y,
                v.x,
                v.y,
                v.length()
            );
        }
        min_x = min_x.min(p.x);
        if p.x < -0.6 || p.y < -1.3 {
            break;
        }
    }
    println!("arch speed={speed} prune={prune}: min_x = {min_x:.3}");
    min_x
}

/// RESTITUTION: drop a ball straight down onto a real wall; return the bounce ratio (rebound speed
/// / impact speed). Metal-like elasticity should give clearly > 0.
fn restitution_ratio(prune: bool) -> f32 {
    let mut app = base_app(prune);
    // A real floor with metal-like elasticity, top surface at y = 0.
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::rectangle(1.0, 0.1),
        Transform::from_xyz(0.0, -0.05, 0.0),
        Restitution::new(0.5),
    ));
    let ball = spawn_ball(&mut app, Vec2::new(0.0, 0.2), Vec2::new(0.0, 0.0));
    let mut impact = 0.0f32;
    let mut rebound = 0.0f32;
    for _ in 0..120 {
        app.update();
        let vy = app.world().get::<LinearVelocity>(ball).unwrap().0.y;
        if vy < impact {
            impact = vy; // most negative = impact speed
        }
        if impact < -0.1 {
            rebound = rebound.max(vy); // largest upward after impact
        }
    }
    let ratio = if impact < 0.0 { rebound / -impact } else { 0.0 };
    println!("restitution prune={prune}: impact={impact} rebound={rebound} ratio={ratio}");
    ratio
}

// --- The bug exists at our scale (pruning off) ---

#[test]
fn skim_kick_reproduces_without_pruning() {
    assert!(
        trimesh_skim_kick(15.0, false) > 1.0,
        "expected a phantom kick without the swept-path pruning"
    );
}

// --- The fix kills the phantom kick ... ---

#[test]
fn pruning_stops_skim_kick() {
    for speed in [5.0, 10.0, 15.0, 20.0] {
        assert!(
            trimesh_skim_kick(speed, true) < 0.05,
            "swept-path pruning should stop the phantom kick at {speed} m/s"
        );
    }
}

// --- ... without killing restitution ---

#[test]
fn pruning_keeps_restitution() {
    assert!(
        restitution_ratio(true) > 0.25,
        "ball must still bounce off a real wall with pruning on"
    );
}

// --- and a converging ball deflects along a curved wall instead of stalling ---
// (The high physics tick rate does the heavy lifting here: one narrow-phase frame spans
// only a few curve segments. At 60 Hz this stalled with or without pruning.)

#[test]
fn ball_rounds_a_curve() {
    let with = arch_min_x(4.0, true);
    assert!(
        with < -0.25,
        "ball should round the arc and keep going left, got min_x {with}"
    );
}
