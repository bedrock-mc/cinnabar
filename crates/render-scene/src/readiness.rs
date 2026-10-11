use bevy::prelude::Resource;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
/// True once every registered variant for the current views has compiled.
#[derive(Resource, Clone, Debug, Default)]
pub struct PipelineWarmupReadiness(Arc<AtomicBool>);

impl PipelineWarmupReadiness {
    /// Publishes whether all registered pipeline variants are ready for the current view.
    pub fn set_ready(&self, ready: bool) {
        self.0.store(ready, Ordering::Release);
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
