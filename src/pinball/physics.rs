//! Shared physics behaviour ported from vpinball.

use bevy::prelude::*;

/// vpinball's elasticity falloff: a rubber-like surface loses elasticity with
/// impact speed, `e_eff = e / (1 + falloff * v)` with `v` the approach speed in
/// m/s (`ElasticityWithFalloff` in vpinball's collide.h; its 18.53 speed units are
/// one m/s). Without it a vpx surface with elasticity 1.0 and a falloff (a common
/// authoring pattern for rubbers) bounces forever instead of settling.
///
/// Applied per contact by the collision hooks (see `pinball::gate`).
#[derive(Component)]
pub(crate) struct ElasticityFalloff(pub(crate) f32);
