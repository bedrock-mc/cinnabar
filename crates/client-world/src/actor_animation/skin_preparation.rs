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
        self.skin_preparation.request(source)
    }

    /// The worker compares replacement bytes against the still-ready result, never against a main-thread hash.
    pub(crate) fn request_replacing_skin_preparation(
        &mut self,
        source: &Arc<protocol::SkinGeometrySource>,
        previous: Option<&Arc<protocol::SkinGeometrySource>>,
    ) -> bool {
        self.skin_preparation.request_replacing(source, previous)
    }

    /// Dispatches one bounded batch through the existing Rayon pool.
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
