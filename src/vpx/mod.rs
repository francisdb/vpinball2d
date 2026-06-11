use bevy::prelude::*;
use loader::VpxLoader;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub mod assets;
mod loader;
mod primitive_mesh;
mod ramp_mesh;
// TODO make this private again after the code has been moved
pub mod triangulate;

pub use assets::*;

/// Progress of the vpx load in flight, written from the asset loader task and read
/// by the loading screen. A vpx is one opaque asset to bevy, so the loader itself
/// counts the items (images, sounds, game item meshes) it has processed. Only one
/// table loads at a time.
#[derive(Resource, Clone, Default)]
pub struct VpxLoadProgress(Arc<ProgressCounters>);

#[derive(Default)]
struct ProgressCounters {
    done: AtomicU32,
    total: AtomicU32,
}

impl VpxLoadProgress {
    /// Forget the previous load, so [`Self::fraction`] reads `None` (and the bar
    /// stays empty) until the next load starts counting.
    pub fn reset(&self) {
        self.0.done.store(0, Ordering::Relaxed);
        self.0.total.store(0, Ordering::Relaxed);
    }

    /// Start counting a new load of `total` items.
    pub(crate) fn start(&self, total: u32) {
        self.0.done.store(0, Ordering::Relaxed);
        self.0.total.store(total, Ordering::Relaxed);
    }

    /// One more item finished loading.
    pub(crate) fn advance(&self) {
        self.0.done.fetch_add(1, Ordering::Relaxed);
    }

    /// Fraction loaded (0..=1), or `None` when no load has started.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.0.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.0.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0))
    }
}

pub struct VpxPlugin;

impl Plugin for VpxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VpxLoadProgress>();
        app.init_asset::<VpxAsset>()
            .preregister_asset_loader::<VpxLoader>(&["vpx"]);
    }
    fn finish(&self, app: &mut App) {
        let progress = app.world().resource::<VpxLoadProgress>().clone();
        app.register_asset_loader(VpxLoader { progress });
    }
}
