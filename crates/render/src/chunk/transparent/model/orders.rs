//! CPU witnesses for the exact model-reference order already uploaded to the GPU.
use super::*;

#[derive(Debug)]
pub(in crate::chunk) struct TransparentModelDrawOrder {
    pub(in crate::chunk) identity: TransparentModelAllocationIdentity,
    pub(in crate::chunk) revision: u64,
    pub(in crate::chunk) words: Arc<[[u32; 2]]>,
    /// Class the uploaded order was sorted for; `None` for the mesher's unsorted order.
    pub(in crate::chunk) class: Option<FaceOrderClass>,
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentModelDrawOrders {
    entries: HashMap<Entity, TransparentModelDrawOrder>,
    next_revision: u64,
    retained_refs: usize,
    // A successful GPU sort with no room for its CPU witness must never be
    // mistaken for a newly uploaded, naturally ordered allocation next frame.
    // Its class is still kept, so a camera move re-sorts it like any witnessed order.
    unwitnessed: HashMap<Entity, (TransparentModelAllocationIdentity, Option<FaceOrderClass>)>,
}

impl TransparentModelDrawOrders {
    pub(in crate::chunk) fn remap_entities(&mut self, residents: &HashMap<SubChunkKey, Entity>) {
        self.entries = self
            .entries
            .drain()
            .filter_map(|(_, mut order)| {
                let &entity = residents.get(&order.identity.key)?;
                order.identity.entity = entity;
                Some((entity, order))
            })
            .collect();
        self.unwitnessed = self
            .unwitnessed
            .drain()
            .filter_map(|(_, (mut identity, class))| {
                let &entity = residents.get(&identity.key)?;
                identity.entity = entity;
                Some((entity, (identity, class)))
            })
            .collect();
        self.retained_refs = self.entries.values().map(|order| order.words.len()).sum();
    }

    pub(in crate::chunk) fn get(
        &self,
        identity: &TransparentModelAllocationIdentity,
    ) -> Option<&TransparentModelDrawOrder> {
        self.entries
            .get(&identity.entity)
            .filter(|entry| &entry.identity == identity)
    }

    /// Class the uploaded order of `identity` was sorted for, witnessed or not.
    pub(in crate::chunk) fn class(
        &self,
        identity: &TransparentModelAllocationIdentity,
    ) -> Option<FaceOrderClass> {
        self.get(identity).map_or_else(
            || {
                self.unwitnessed
                    .get(&identity.entity)
                    .filter(|(unwitnessed, _)| unwitnessed == identity)
                    .and_then(|(_, class)| *class)
            },
            |order| order.class,
        )
    }

    fn insert(
        &mut self,
        identity: TransparentModelAllocationIdentity,
        words: Arc<[[u32; 2]]>,
        class: Option<FaceOrderClass>,
    ) {
        let replaced = self
            .entries
            .get(&identity.entity)
            .map_or(0, |entry| entry.words.len());
        let retained = self
            .retained_refs
            .saturating_sub(replaced)
            .saturating_add(words.len());
        if retained > MAX_TRANSPARENT_DRAW_REFS {
            // Never leave a stale witness after a successful GPU write.
            self.entries.remove(&identity.entity);
            self.unwitnessed.insert(identity.entity, (identity, class));
            self.retained_refs = self.retained_refs.saturating_sub(replaced);
            return;
        }
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        self.unwitnessed.remove(&identity.entity);
        self.entries.insert(
            identity.entity,
            TransparentModelDrawOrder {
                identity,
                revision: self.next_revision,
                words,
                class,
            },
        );
        self.retained_refs = retained;
    }

    pub(in crate::chunk) fn publish(
        &mut self,
        address: &TransparentModelAddressIdentity,
        batch: TransparentModelSortBatch,
    ) {
        let Some(identity) = address
            .allocations
            .iter()
            .find(|identity| identity.draw_range == batch.draw_range)
        else {
            return;
        };
        if batch.words.len().checked_mul(2)
            != batch
                .draw_range
                .end
                .checked_sub(batch.draw_range.start)
                .map(|count| count as usize)
        {
            return;
        }
        self.insert(identity.clone(), Arc::from(batch.words), Some(batch.class));
    }

    /// Records `batch`'s class without a new revision when its order is already uploaded.
    pub(in crate::chunk) fn reclassify_unchanged(
        &mut self,
        address: &TransparentModelAddressIdentity,
        batch: &TransparentModelSortBatch,
    ) -> bool {
        let Some(identity) = address
            .allocations
            .iter()
            .find(|identity| identity.draw_range == batch.draw_range)
        else {
            return false;
        };
        let Some(entry) = self
            .entries
            .get_mut(&identity.entity)
            .filter(|entry| &entry.identity == identity && *entry.words == *batch.words)
        else {
            return false;
        };
        entry.class = Some(batch.class);
        true
    }

    pub(in crate::chunk) fn refresh(
        &mut self,
        arena: &ChunkGpuArena,
        instances: &Query<&ChunkRenderInstance>,
    ) {
        self.unwitnessed.retain(|entity, (identity, _)| {
            arena.allocations.get(entity).is_some_and(|resident| {
                let gpu = &resident.gpu;
                gpu.generation == identity.generation
                    && gpu.key == identity.key
                    && gpu.model_range.as_ref() == Some(&identity.model_range)
                    && gpu.transparent_model_draw_range.as_ref() == Some(&identity.draw_range)
            })
        });
        self.entries.retain(|entity, order| {
            arena.allocations.get(entity).is_some_and(|resident| {
                let gpu = &resident.gpu;
                gpu.generation == order.identity.generation
                    && gpu.key == order.identity.key
                    && gpu.model_range.as_ref() == Some(&order.identity.model_range)
                    && gpu.transparent_model_draw_range.as_ref() == Some(&order.identity.draw_range)
            })
        });
        self.retained_refs = self
            .entries
            .values()
            .map(|entry| entry.words.len())
            .sum::<usize>();
        for (&entity, resident) in &arena.allocations {
            if self.entries.contains_key(&entity) || self.unwitnessed.contains_key(&entity) {
                continue;
            }
            let gpu = &resident.gpu;
            let (Some(model_range), Some(draw_range)) = (
                gpu.model_range.as_ref(),
                gpu.transparent_model_draw_range.as_ref(),
            ) else {
                continue;
            };
            let Ok(instance) = instances.get(entity) else {
                continue;
            };
            if !transparent_model_allocation_matches(instance, gpu)
                || !model_range.start.is_multiple_of(4)
                || !draw_range.start.is_multiple_of(2)
                || instance.transparent_model_draw_refs.len().checked_mul(2)
                    != Some(draw_range.end.saturating_sub(draw_range.start) as usize)
            {
                continue;
            }
            if self
                .retained_refs
                .saturating_add(instance.transparent_model_draw_refs.len())
                > MAX_TRANSPARENT_DRAW_REFS
            {
                continue;
            }
            let Some(words) = instance
                .transparent_model_draw_refs
                .iter()
                .map(|draw| {
                    let [record, quad] = draw.words();
                    Some([record.checked_add(model_range.start / 4)?, quad])
                })
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            self.insert(
                TransparentModelAllocationIdentity {
                    entity,
                    key: gpu.key,
                    generation: gpu.generation,
                    model_range: model_range.clone(),
                    draw_range: draw_range.clone(),
                },
                words.into(),
                None,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> TransparentModelAllocationIdentity {
        TransparentModelAllocationIdentity {
            entity: Entity::from_bits(1),
            key: SubChunkKey::new(0, 0, 0, 0),
            generation: 7,
            model_range: 0..4,
            draw_range: 4..8,
        }
    }

    #[test]
    fn upload_publishes_its_exact_order_before_other_chunk_batches_complete() {
        let identity = identity();
        let address = TransparentModelAddressIdentity {
            asset_identity: ChunkTextureAssetIdentity::new(0, 1),
            allocations: Arc::from([identity.clone()]),
        };
        let mut orders = TransparentModelDrawOrders::default();
        orders.insert(identity.clone(), Arc::from([[0, 0], [0, 1]]), None);
        let initial_revision = orders.get(&identity).unwrap().revision;
        orders.publish(
            &address,
            TransparentModelSortBatch {
                draw_range: identity.draw_range.clone(),
                class: FaceOrderClass::Far([0, 0, 1]),
                words: vec![[0, 1], [0, 0]].into_boxed_slice(),
            },
        );
        let uploaded = orders.get(&identity).unwrap();
        assert_eq!(uploaded.words.as_ref(), [[0, 1], [0, 0]]);
        assert_ne!(uploaded.revision, initial_revision);
        let mut replacement = identity;
        replacement.generation += 1;
        assert!(orders.get(&replacement).is_none());
    }

    #[test]
    fn witness_ceiling_keeps_successfully_sorted_but_unwitnessed_identity() {
        let identity = identity();
        let mut orders = TransparentModelDrawOrders {
            // Exercise the hard counter boundary without allocating its full buffer.
            retained_refs: MAX_TRANSPARENT_DRAW_REFS,
            ..default()
        };
        orders.insert(identity.clone(), Arc::from([[0, 1], [0, 0]]), None);
        assert!(orders.get(&identity).is_none());
        assert_eq!(
            orders.unwitnessed.get(&identity.entity),
            Some(&(identity.clone(), None))
        );
        assert_eq!(orders.retained_refs, MAX_TRANSPARENT_DRAW_REFS);
    }

    #[test]
    fn candidate_publication_remaps_both_sorted_and_unwitnessed_entities_by_world_key() {
        let temporary = identity();
        let mut unknown = temporary.clone();
        unknown.entity = Entity::from_bits(2);
        unknown.key.x += 1;
        let mut orders = TransparentModelDrawOrders::default();
        orders.insert(temporary.clone(), Arc::from([[0, 1], [0, 0]]), None);
        orders
            .unwitnessed
            .insert(unknown.entity, (unknown.clone(), None));
        let resident = Entity::from_bits(20);
        let unknown_resident = Entity::from_bits(21);
        let revision = orders.get(&temporary).unwrap().revision;
        orders.remap_entities(&HashMap::from([
            (temporary.key, resident),
            (unknown.key, unknown_resident),
        ]));
        assert!(orders.get(&temporary).is_none());
        let mut published = temporary;
        published.entity = resident;
        let witness = orders.get(&published).unwrap();
        assert_eq!(witness.words.as_ref(), [[0, 1], [0, 0]]);
        assert_eq!(witness.revision, revision);
        assert_eq!(orders.retained_refs, witness.words.len());
        unknown.entity = unknown_resident;
        assert_eq!(
            orders.unwitnessed.get(&unknown_resident),
            Some(&(unknown, None))
        );
    }
}
