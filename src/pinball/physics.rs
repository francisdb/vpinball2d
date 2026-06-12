//! Shared physics behaviour ported from vpinball.

use bevy::prelude::*;

/// vpinball speed units (VP units per 10 ms physics tick) per m/s: velocity deltas
/// from bumpers/slingshots/kickers are authored in these (18.53 units = 1 m/s).
pub(crate) const VP_SPEED_TO_M_S: f32 = 1.0 / 18.53;

/// vpinball's elasticity falloff: a rubber-like surface loses elasticity with
/// impact speed, `e_eff = e / (1 + falloff * v)` with `v` the approach speed in
/// m/s (`ElasticityWithFalloff` in vpinball's collide.h; its 18.53 speed units are
/// one m/s). Without it a vpx surface with elasticity 1.0 and a falloff (a common
/// authoring pattern for rubbers) bounces forever instead of settling.
///
/// Applied per contact by the collision hooks (see `pinball::gate`).
#[derive(Component)]
pub(crate) struct ElasticityFalloff(pub(crate) f32);
