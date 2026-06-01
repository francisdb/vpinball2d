//! Play interface for headless-but-windowed debugging.
//!
//! The game runs as a normal window (so a human can watch it), while this module exposes a text
//! control + telemetry channel so an operator who cannot see the pixels (e.g. an AI agent, or
//! anyone over a terminal) can still drive and observe it. The two halves are independent Cargo
//! features, so a build can enable either or both (`dev` enables both):
//!
//!   - `control` (`remote_control` feature): read newline-separated commands from a file each
//!     frame and inject them as keyboard / ball input (`tp`, `launch`, `clear`, flipper/plunger
//!     `hold`/`release`/`tap`, `nudge`).
//!   - `telemetry` (`telemetry` feature): write the game state as JSON (a ~50 Hz overwrite
//!     snapshot plus an append-only stream) and append a line per ball/object contact to an
//!     event log. Read-only observation.

use bevy::prelude::*;

#[cfg(feature = "remote_control")]
mod control;
#[cfg(feature = "telemetry")]
mod telemetry;

pub(super) fn plugin(app: &mut App) {
    #[cfg(feature = "remote_control")]
    control::plugin(app);
    #[cfg(feature = "telemetry")]
    telemetry::plugin(app);
}
