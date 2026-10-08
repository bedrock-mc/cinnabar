//! The actor store drives bounded immutable preparation before choosing complete profiles.
use super::*;

impl ActorAnimationStore {
    /// Diagnostic stores have no catalog against which a custom model can be resolved.
    pub(crate) fn has_skin_preparation(&self) -> bool {
        self.assets.is_some()
    }

    /// Receives bounded worker results without waiting for an unfinished job.
    pub(crate) fn begin_skin_preparation(&mut self) {
        self.skin_preparation.begin_frame();
    }

    /// Admits only a retained source pointer; all content work belongs to the worker.
    pub(crate) fn request_skin_preparation(
        &mut self,
        source: &Arc<protocol::SkinGeometrySource>,
    ) -> bool {
        self.request_replacing_skin_preparation(source, None)
    }

    /// The worker compares replacement bytes against the still-ready result, never against a main-thread hash.
    pub(crate) fn request_replacing_skin_preparation(
        &mut self,
        source: &Arc<protocol::SkinGeometrySource>,
        previous: Option<&Arc<protocol::SkinGeometrySource>>,
    ) -> bool {
        let Some(assets) = &self.assets else {
            return false;
        };
        self.skin_preparation
            .request_replacing(source, previous, assets)
    }

    /// Installs a published appearance's model now rather than at the rig's next tick.
    pub(crate) fn sync_skin_model(
        &mut self,
        runtime_id: u64,
        source: Option<&Arc<protocol::SkinGeometrySource>>,
    ) {
        let Some(state) = self
            .runtime_to_lifetime
            .get(&runtime_id)
            .and_then(|lifetime| self.rigs.get_mut(lifetime))
        else {
            return;
        };
        if skin::sync_skin(state, source, &self.skin_preparation) {
            self.stats.invalid_skin_geometries = self.stats.invalid_skin_geometries.saturating_add(1);
        }
    }

    /// Dispatches one bounded batch through the existing Rayon pool, beside any still running.
    pub(crate) fn submit_skin_preparation(&mut self) {
        if let Some(assets) = &self.assets {
            self.skin_preparation.submit(assets);
        }
    }

    /// Tests can await their own submitted batch without making a timing assertion.
    #[cfg(any(test, feature = "appearance-test-support"))]
    pub(crate) fn finish_skin_preparation_for_test(&mut self) {
        self.skin_preparation.finish_for_test();
    }
}
