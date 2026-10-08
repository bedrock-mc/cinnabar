use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bevy::prelude::Resource;

/// Readiness of all actor raster contracts for the active render views.
#[derive(Clone, Debug, Default, Resource)]
pub struct ActorPipelineReadiness {
    ready: Arc<AtomicBool>,
}

impl ActorPipelineReadiness {
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub(crate) fn publish(&self, ready: bool) {
        self.ready.store(ready, Ordering::Release);
    }
}
