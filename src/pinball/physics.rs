//! App-wide fix for avian's speculative "ghost" contacts at our real (small) scale.
//!
//! avian generates speculative contacts within `velocity * dt` of a collider, so a fast ball gets
//! an impulse from a wall it is merely near - including one buried inside another wall (the
//! eject-hole corner, ~1.5 mm deep). At real pinball scale (27 mm ball) that margin (~mm at a few
//! m/s) is comparable to such features, so the ball bounces off walls it never touches. Bounding
//! the global speculative margin removes the phantom bounce but guts restitution, because avian
//! derives restitution from the velocity-clamped normal speed.
//!
//! Instead we drop the phantom contacts directly: a real touch has ~zero separation and is kept; a
//! phantom/buried contact keeps its gap and is dropped. Velocities are never touched, so restitution
//! survives. The ball's [`SweptCcd`] still covers genuinely fast hits.
//!
//! avian allows a single [`CollisionHooks`] per app, so rather than its own hook this is a helper
//! ([`contact_is_real`]) that [`GateCollisionHooks`](super::gate::GateCollisionHooks) applies to
//! every non-gate contact. Verified deterministically in `tests/ghost_collision.rs`.

use avian2d::prelude::*;

/// Keep contacts only when within this separation of touching (metres): below the smallest
/// buried-wall offset (~1.5 mm), above ~0 so genuine touches are kept.
const MAX_CONTACT_SEPARATION: f32 = 0.0005;

/// Whether a contact is a real touch (keep it) rather than a phantom speculative contact (drop it):
/// some manifold point must be within `MAX_CONTACT_SEPARATION` of touching. `penetration` is
/// positive when overlapping, negative (a gap) when merely speculative.
pub(crate) fn contact_is_real(contacts: &ContactPair) -> bool {
    contacts.manifolds.iter().any(|manifold| {
        manifold
            .points
            .iter()
            .any(|point| point.penetration > -MAX_CONTACT_SEPARATION)
    })
}
