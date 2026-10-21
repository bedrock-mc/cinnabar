//! Retained identities and camera-independent centroids for visible model allocations.

use super::*;

struct ModelCentroid {
    centroid: Vec3,
    words: [u32; 2],
}

struct AllocationPage {
    identity: TransparentModelAllocationIdentity,
    model_refs: Arc<[PackedModelRef]>,
    model_lighting: Arc<[meshing::PackedQuadLighting]>,
    draw_refs: Arc<[PackedModelDrawRef]>,
    centroids: Option<Arc<[ModelCentroid]>>,
    seen: bool,
}

impl AllocationPage {
    /// Retains every mesh stream whose pointer identifies the cached centroid inputs.
    fn new(identity: TransparentModelAllocationIdentity, instance: &ChunkRenderInstance) -> Self {
        Self {
            identity,
            model_refs: Arc::clone(&instance.model_refs),
            model_lighting: Arc::clone(&instance.model_lighting),
            draw_refs: Arc::clone(&instance.transparent_model_draw_refs),
            centroids: None,
            seen: true,
        }
    }

    /// Requires the same GPU allocation and the same immutable mesh streams.
    fn matches(
        &self,
        identity: &TransparentModelAllocationIdentity,
        instance: &ChunkRenderInstance,
    ) -> bool {
        self.identity == *identity
            && Arc::ptr_eq(&self.model_refs, &instance.model_refs)
            && Arc::ptr_eq(&self.model_lighting, &instance.model_lighting)
            && Arc::ptr_eq(&self.draw_refs, &instance.transparent_model_draw_refs)
    }
}

#[cfg(test)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct PreparationCounts {
    addresses: usize,
    pages: usize,
    centroids: usize,
}

#[derive(Default)]
pub(super) struct TransparentModelPreparationCache {
    pages: HashMap<Entity, AllocationPage>,
    assets: Option<ChunkTextureAssets>,
    address: Option<TransparentModelAddressIdentity>,
    visible: Vec<Entity>,
    observed: Vec<Entity>,
    revision: u64,
    dirty: bool,
    invalidated: HashSet<Entity>,
    invalidate_all: bool,
    #[cfg(test)]
    counts: PreparationCounts,
}

impl TransparentModelPreparationCache {
    /// Drops abandoned pages while keeping future revisions distinct from old worker keys.
    pub(super) fn clear(&mut self) {
        self.pages.clear();
        self.assets = None;
        self.address = None;
        self.visible.clear();
        self.observed.clear();
        self.revision = self.revision.wrapping_add(1);
        self.dirty = false;
        self.invalidated.clear();
        self.invalidate_all = true;
    }

    /// Starts a visibility scan retaining only the carrier used by geometry.
    /// Carrier changes invalidate every centroid page.
    pub(super) fn begin_frame(&mut self, assets: &ChunkTextureAssets) {
        if self.assets.as_ref().map(ChunkTextureAssets::identity) != Some(assets.identity()) {
            self.invalidate_all |= self.assets.is_some();
            self.pages.clear();
            self.assets = Some(assets.with_updated_enhanced(None));
            self.dirty = true;
        }
        self.observed.clear();
        for page in self.pages.values_mut() {
            page.seen = false;
        }
    }

    /// Reuses an allocation's identity and centroid page until one of its inputs changes.
    pub(super) fn observe(
        &mut self,
        identity: TransparentModelAllocationIdentity,
        instance: &ChunkRenderInstance,
    ) {
        self.observed.push(identity.entity);
        if let Some(page) = self.pages.get_mut(&identity.entity) {
            if page.matches(&identity, instance) {
                page.seen = true;
                return;
            }
            self.invalidated.insert(identity.entity);
        } else if self.address.is_some() {
            // An evicted page can return with a different mesh at the same GPU address.
            self.invalidated.insert(identity.entity);
        }
        self.dirty = true;
        self.pages
            .insert(identity.entity, AllocationPage::new(identity, instance));
    }

    /// Publishes a stable identity order only after a complete, valid visibility scan.
    pub(super) fn finish_frame(
        &mut self,
        orders: &mut TransparentModelDrawOrders,
    ) -> TransparentModelAddressIdentity {
        let previous_len = self.pages.len();
        self.pages.retain(|_, page| page.seen);
        self.dirty |= self.pages.len() != previous_len || self.visible != self.observed;
        if self.dirty || self.address.is_none() {
            let mut identities = self
                .observed
                .iter()
                .map(|entity| self.pages[entity].identity.clone())
                .collect::<Vec<_>>();
            // Stable ties follow visibility order, as in the uncached preparation path.
            identities.sort_by_key(|identity| (identity.key, identity.draw_range.start));
            if self.address.is_some() {
                self.revision = self.revision.wrapping_add(1);
            }
            let assets = self.assets.as_ref().expect("a frame retains its assets");
            self.address = Some(TransparentModelAddressIdentity {
                asset_identity: assets.identity(),
                allocations: identities.into(),
                revision: self.revision,
                _retained_assets: Some(Arc::clone(assets.assets())),
            });
            std::mem::swap(&mut self.visible, &mut self.observed);
            self.dirty = false;
            #[cfg(test)]
            {
                self.counts.addresses += 1;
            }
        }
        orders.invalidate_classes(&self.invalidated, self.invalidate_all);
        self.invalidated.clear();
        self.invalidate_all = false;
        self.address.as_ref().unwrap().clone()
    }

    /// Resolves only missing pages and reuses the combined array for an unchanged address.
    pub(super) fn candidates(
        &mut self,
        combined: &mut Option<TransparentModelCandidateCache>,
        address: &TransparentModelAddressIdentity,
        total_refs: usize,
        model_templates: &[ModelTemplate],
        model_quads: &[assets::ModelQuad],
    ) -> Option<Arc<[TransparentModelSortCandidate]>> {
        if let Some(cache) = combined.as_ref().filter(|cache| cache.address == *address) {
            return Some(Arc::clone(&cache.candidates));
        }
        let mut candidates = Vec::with_capacity(total_refs);
        for identity in address.allocations.iter() {
            let page = self.pages.get_mut(&identity.entity)?;
            if page.centroids.is_none() {
                if page.draw_refs.len().checked_mul(2)
                    != Some(
                        identity
                            .draw_range
                            .end
                            .saturating_sub(identity.draw_range.start)
                            as usize,
                    )
                {
                    return None;
                }
                #[cfg(test)]
                {
                    self.counts.pages += 1;
                }
                let mut resolved = Vec::with_capacity(page.draw_refs.len());
                for draw_ref in page.draw_refs.iter().copied() {
                    #[cfg(test)]
                    {
                        self.counts.centroids += 1;
                    }
                    let (centroid, words) = transparent_model_draw_candidate(
                        identity.key,
                        &page.model_refs,
                        &page.model_lighting,
                        draw_ref,
                        model_templates,
                        model_quads,
                        identity.model_range.start / 4,
                    )?;
                    resolved.push(ModelCentroid { centroid, words });
                }
                page.centroids = Some(resolved.into());
            }
            for (stable_index, centroid) in page.centroids.as_ref()?.iter().enumerate() {
                candidates.push(TransparentModelSortCandidate {
                    entity: identity.entity,
                    key: identity.key,
                    draw_range: identity.draw_range.clone(),
                    stable_index: u32::try_from(stable_index).ok()?,
                    centroid: centroid.centroid,
                    words: centroid.words,
                });
            }
        }
        let candidates = Arc::from(candidates);
        *combined = Some(TransparentModelCandidateCache {
            address: address.clone(),
            candidates: Arc::clone(&candidates),
        });
        Some(candidates)
    }
}

#[cfg(test)]
mod tests;
