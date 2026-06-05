//! Table nudge.
//!
//! Modelled as a virtual "table on springs": a tap shoves the table and it rings
//! back to centre (damped). Rather than physically moving the walls, we work in
//! the table's reference frame:
//!   - physics: the table's acceleration becomes a fictitious force on every
//!     dynamic body, applied through the global gravity vector. The shove is a
//!     short, decaying force, so the table returns to rest and the fictitious
//!     force integrates to zero: a free ball just wiggles and returns to its
//!     trajectory, while a ball resting against a wall/flipper gets a net kick
//!     (one-sided contacts only pass the away-half). The shove *accumulates*, so
//!     several nudges in quick succession simply add up instead of clobbering
//!     each other, and rapid taps can't build a sustained one-way push.
//!   - visual: the camera is offset by the table's displacement so the playfield
//!     appears to jolt (purely cosmetic).

use crate::PausableSystems;
use crate::screens::Screen;
use avian2d::math::Vector;
use avian2d::prelude::*;
use bevy::camera::Camera2d;
use bevy::prelude::*;
use core::f32::consts::TAU;

// How fast the "table on springs" rings back to centre.
const NUDGE_FREQUENCY_HZ: f32 = 8.0;
// < 1.0 wobbles then settles; ~0.5 gives a visible shake.
const NUDGE_DAMPING_RATIO: f32 = 0.5;
// Peak shove acceleration from one tap, in m/s^2. This world is in metres, so it
// is the Pinball2DAvian value (12000 px/s^2) divided by its 492.3 px/m scale.
const NUDGE_PUSH_ACCEL: f32 = 24.4;
// The shove decays over roughly this long (seconds).
const NUDGE_PUSH_DECAY: f32 = 0.04;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Nudge>()
        .add_systems(Startup, capture_base_gravity)
        // Read input every render frame, but step the oscillator and set gravity in
        // FixedUpdate so it runs in lockstep with Avian's fixed-timestep physics
        // (FixedPostUpdate); otherwise the ringing gravity gets aliased by the
        // physics step and the nudge doesn't cancel on a free ball.
        .add_systems(
            Update,
            nudge_input
                .in_set(PausableSystems)
                .run_if(in_state(Screen::Gameplay)),
        )
        .add_systems(FixedUpdate, apply_nudge);
}

#[derive(Resource, Default)]
struct Nudge {
    base_gravity: Vector,
    /// Virtual table displacement and velocity (m, m/s).
    pos: Vec2,
    vel: Vec2,
    /// Current shove acceleration on the table; accumulates per tap, then decays.
    force: Vec2,
}

fn capture_base_gravity(mut nudge: ResMut<Nudge>, gravity: Res<Gravity>) {
    nudge.base_gravity = gravity.0;
}

fn nudge_input(keyboard: Res<ButtonInput<KeyCode>>, mut nudge: ResMut<Nudge>) {
    // Keys mirror Visual Pinball defaults: Z = left, / = right, Space = center.
    // The value is the direction we want the BALL to lurch; the table is shoved the
    // opposite way. Flip a sign here if a direction feels backwards.
    let mut ball_dir = Vec2::ZERO;
    if keyboard.just_pressed(KeyCode::KeyZ) {
        ball_dir.x -= 1.0; // left nudge
    }
    if keyboard.just_pressed(KeyCode::Slash) {
        ball_dir.x += 1.0; // right nudge
    }
    if keyboard.just_pressed(KeyCode::Space) {
        ball_dir.y -= 1.0; // center nudge: jolt the table up (ball lurches down)
    }

    if ball_dir != Vec2::ZERO {
        // Accumulate so overlapping nudges add instead of overwriting.
        nudge.force += -ball_dir.normalize() * NUDGE_PUSH_ACCEL;
    }
}

fn apply_nudge(
    time: Res<Time>,
    mut nudge: ResMut<Nudge>,
    mut gravity: ResMut<Gravity>,
    mut cameras: Query<&mut Transform, (With<Camera2d>, Without<super::lightmap::LightmapCamera>)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }

    let omega = TAU * NUDGE_FREQUENCY_HZ;
    let stiffness = omega * omega;
    let damping = 2.0 * NUDGE_DAMPING_RATIO * omega;

    // Damped spring driven by the (decaying) shove: table_accel = force - k*x - c*v.
    let table_accel = nudge.force - stiffness * nudge.pos - damping * nudge.vel;
    let vel = nudge.vel + table_accel * dt;
    nudge.vel = vel;
    nudge.pos += vel * dt;

    // Decay the shove so it stays transient and the table returns to rest.
    nudge.force *= (-dt / NUDGE_PUSH_DECAY).exp();

    // Fictitious acceleration felt by every body in the table's frame.
    gravity.0 = nudge.base_gravity - table_accel;

    // Fake the table jolting by offsetting the camera (visual only).
    for mut camera in &mut cameras {
        camera.translation.x = -nudge.pos.x;
        camera.translation.y = -nudge.pos.y;
    }
}
