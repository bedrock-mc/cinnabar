//! Composition results retained independently for each geometry and tick endpoint.
use super::*;

#[derive(Debug, Default)]
pub(crate) struct Compositions {
    poses: BTreeMap<(Option<u32>, bool), ComposedPose>,
}

/// One geometry endpoint's local channels and the matrices they produced.
#[derive(Debug, Default)]
struct ComposedPose {
    local: Vec<pose::LocalDelta>,
    model: Arc<[BoneTransform]>,
}

impl Compositions {
    /// Avoids reconstructing matrices when sampled local channels are identical.
    pub(crate) fn compose(
        &mut self,
        geometry: Option<u32>,
        previous: bool,
        count: usize,
        local: &[pose::LocalDelta],
        compose: impl FnOnce() -> Option<Vec<BoneTransform>>,
    ) -> Option<Arc<[BoneTransform]>> {
        if !super::cache::enabled() {
            return compose().map(Arc::from);
        }
        let entry = self.poses.entry((geometry, previous)).or_default();
        if entry.local.len() == local.len()
            && entry.local.iter().zip(local).all(|(a, b)| a.same_bits(*b))
            && entry.model.len() == count
        {
            return Some(Arc::clone(&entry.model));
        }
        let composed = Arc::from(compose()?);
        entry.local.clear();
        entry.local.extend_from_slice(local);
        entry.model = composed;
        Some(Arc::clone(&entry.model))
    }
}
