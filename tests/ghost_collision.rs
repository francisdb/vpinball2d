//! Headless, tick-driven physics tests for the "ball bounces off a wall it does not touch" bug and
//! its fix.
//!
//! Two isolated scenarios, both at our real (tiny) scale:
//! - `ghost`: a bouncy "metal" wall buried inside a "wooden" wall; a ball sliding down the wooden
//!   surface must NOT bounce off the buried wall it never reaches.
//! - `restitution`: a ball dropped onto a real wall MUST bounce (the fix must not kill bouncing).
//!
//! The bug is avian's speculative contacts: a fast ball gets an impulse from a wall it is only
//! *near* (within `velocity * dt`), even one buried behind another. Bounding the global speculative
//! margin "fixes" the ghost but guts restitution (avian derives restitution from the
//! velocity-clamped normal speed). Instead we reject phantom contacts with a [`CollisionHooks`]:
//! drop a contact while the ball is still separated from the collider by more than
//! [`MaxContactSeparation`]. A real touch has ~zero separation and is kept; a buried/phantom contact
//! keeps its gap and is dropped. Velocities are never touched, so restitution survives.

use avian2d::math::Vector;
use avian2d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

const STEP: f64 = 1.0 / 60.0;
const BALL_RADIUS: f32 = 0.0135;
const BURIED_DEPTH: f32 = 0.0015;
/// Effectively disables the separation filter (keeps all contacts).
const FILTER_OFF: f32 = 1.0;
/// Drop contacts separated by more than this. Below the buried depth, above ~zero for real touches.
const FILTER_ON: f32 = 0.0005;

/// Drop a contact once the colliders are separated by more than this distance (metres).
#[derive(Resource, Clone, Copy)]
struct MaxContactSeparation(f32);

#[derive(SystemParam)]
struct SeparationHooks<'w> {
    max_separation: Res<'w, MaxContactSeparation>,
}

impl CollisionHooks for SeparationHooks<'_> {
    fn modify_contacts(&self, contacts: &mut ContactPair, _commands: &mut Commands) -> bool {
        let max = self.max_separation.0;
        // Keep the contact only if some point is within `max` of touching (penetration > -max).
        // `penetration` is positive when overlapping, negative (a gap) when separated.
        contacts
            .manifolds
            .iter()
            .any(|m| m.points.iter().any(|p| p.penetration > -max))
    }
}

fn base_app(max_separation: f32) -> App {
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
            .with_collision_hooks::<SeparationHooks>(),
    );
    app.finish();
    app.cleanup();
    app.insert_resource(MaxContactSeparation(max_separation));
    app.insert_resource(Gravity(Vector::NEG_Y * 9.81 * 0.12192));
    app.insert_resource(SubstepCount(50));
    app.insert_resource(Time::<Fixed>::from_hz(60.0));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        STEP,
    )));
    app
}

fn spawn_ball(app: &mut App, pos: Vec2, vel: Vec2) -> Entity {
    app.world_mut()
        .spawn((
            RigidBody::Dynamic,
            Collider::circle(BALL_RADIUS),
            Restitution::new(1.0).with_combine_rule(CoefficientCombine::Min),
            Friction::new(1.0).with_combine_rule(CoefficientCombine::Min),
            SweptCcd::default(),
            // Enable the contact-filter hook for this ball's contacts.
            ActiveCollisionHooks::MODIFY_CONTACTS,
            Transform::from_xyz(pos.x, pos.y, 0.0),
            LinearVelocity(Vector::new(vel.x, vel.y)),
        ))
        .id()
}

/// GHOST: ball slides down the wooden surface past a buried bouncy wall. Returns its max upward vy
/// (a phantom bounce makes it strongly positive).
fn ghost_max_upward_vy(speed: f32, max_separation: f32) -> f32 {
    let mut app = base_app(max_separation);
    // Wooden wall: surface at x = 0, body to the right, non-bouncy.
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::rectangle(0.1, 1.0),
        Transform::from_xyz(0.05, 0.0, 0.0),
        Restitution::new(0.0),
    ));
    // Buried bouncy wall: near edge 1.5 mm behind the surface.
    let w = 0.09;
    app.world_mut().spawn((
        RigidBody::Static,
        Collider::rectangle(w, 0.2),
        Transform::from_xyz(BURIED_DEPTH + w / 2.0, 0.0, 0.0),
        Restitution::new(0.9),
    ));
    let ball = spawn_ball(
        &mut app,
        Vec2::new(-BALL_RADIUS, 0.3),
        Vec2::new(0.2, -speed),
    );
    let mut max_vy = f32::MIN;
    for _ in 0..40 {
        app.update();
        max_vy = max_vy.max(app.world().get::<LinearVelocity>(ball).unwrap().0.y);
        if app.world().get::<Transform>(ball).unwrap().translation.y < -0.3 {
            break;
        }
    }
    println!("ghost speed={speed} max_sep={max_separation}: max upward vy = {max_vy}");
    max_vy
}

/// RESTITUTION: drop a ball straight down onto a real wall; return the bounce ratio (rebound speed
/// / impact speed). Metal-like elasticity should give clearly > 0.
fn restitution_ratio(max_separation: f32) -> f32 {
    let mut app = base_app(max_separation);
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
    println!(
        "restitution max_sep={max_separation}: impact={impact} rebound={rebound} ratio={ratio}"
    );
    ratio
}

// --- The bug exists at our scale (filter off) ---

#[test]
fn ghost_bug_reproduces_without_filter() {
    assert!(
        ghost_max_upward_vy(5.0, FILTER_OFF) > 1.0,
        "expected the bug without the filter"
    );
}

// --- The fix kills the ghost ... ---

#[test]
fn filter_stops_ghost_bounce() {
    assert!(
        ghost_max_upward_vy(5.0, FILTER_ON) < 0.2,
        "filter should stop the phantom bounce"
    );
}

// --- ... without killing restitution ---

#[test]
fn filter_keeps_restitution() {
    assert!(
        restitution_ratio(FILTER_ON) > 0.25,
        "ball must still bounce off a real wall with the filter on"
    );
}
