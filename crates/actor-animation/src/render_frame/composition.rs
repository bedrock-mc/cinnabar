//! Composition results retained independently for each geometry and tick endpoint.
use super::*;

#[derive(Debug, Default)]
pub(crate) struct Compositions {
    poses: BTreeMap<(Option<u32>, bool), (Vec<pose::LocalDelta>, Arc<[BoneTransform]>)>,
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
        if entry.0.len() == local.len()
            && entry.0.iter().zip(local).all(|(a, b)| a.same_bits(*b))
            && entry.1.len() == count
        {
            return Some(Arc::clone(&entry.1));
        }
        let composed = Arc::from(compose()?);
        entry.0.clear();
        entry.0.extend_from_slice(local);
        entry.1 = composed;
        Some(Arc::clone(&entry.1))
    }
}
