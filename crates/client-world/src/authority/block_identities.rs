use std::{collections::HashMap, sync::OnceLock};

use super::*;

impl WorldAuthority {
    /// Retains canonical custom state identities independently of visual pack compilation.
    /// Sequential identities require the admitted internal range; hashed identities use their hash.
    pub fn set_custom_block_identities(&mut self, definitions: &CustomBlocks) {
        let mut identities = HashMap::new();
        self.block_names.clear_custom();
        let mut next = self.custom_block_ids.start;
        let mut skipped = 0_usize;
        for (index, block) in definitions.blocks.iter().enumerate() {
            let first = next;
            if self.network_id_mode == NetworkIdMode::Sequential {
                let Some(end) = next.checked_add(block.state_count) else {
                    skipped = skipped.saturating_add(definitions.blocks.len() - index);
                    break;
                };
                next = end;
                if end > self.custom_block_ids.end {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                self.block_names
                    .custom
                    .entry(Arc::clone(&block.name))
                    .or_default()
                    .extend(first..end);
            }
            let states = block.hashed_states();
            if states.len() != block.state_count as usize {
                skipped = skipped.saturating_add(1);
                continue;
            }
            for (offset, state) in states.into_iter().enumerate() {
                let internal = match self.network_id_mode {
                    NetworkIdMode::Sequential => first + offset as u32,
                    NetworkIdMode::Hashed => state.hash,
                };
                identities.entry(state.hash).or_insert(internal);
                if self.network_id_mode == NetworkIdMode::Hashed {
                    self.block_names
                        .custom
                        .entry(Arc::clone(&block.name))
                        .or_default()
                        .push(internal);
                }
            }
        }
        eprintln!(
            "SESSION_CUSTOM_BLOCK_IDENTITIES session={} mode={:?} states={} skipped_definitions={skipped}",
            self.actor_session_id,
            self.network_id_mode,
            identities.len(),
        );
        self.custom_block_identities = Arc::new(identities);
    }
}

type NameIndex = HashMap<Box<str>, Box<[u32]>>;
type SharedNameIndex = Arc<OnceLock<NameIndex>>;

/// Names resolve to internal store identities, including all admitted custom states.
#[derive(Debug)]
pub(super) struct BlockNames {
    base: SharedNameIndex,
    custom: HashMap<Arc<str>, Vec<u32>>,
    revision: u64,
}

impl BlockNames {
    /// Removes names from the previous custom range before a new palette is admitted.
    pub(super) fn clear_custom(&mut self) {
        self.custom.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    /// Shares the immutable pinned registry index between sessions of the same ID mode.
    pub(super) fn new(mode: NetworkIdMode) -> Self {
        static SEQUENTIAL: OnceLock<SharedNameIndex> = OnceLock::new();
        static HASHED: OnceLock<SharedNameIndex> = OnceLock::new();
        let cache = match mode {
            NetworkIdMode::Sequential => &SEQUENTIAL,
            NetworkIdMode::Hashed => &HASHED,
        };
        Self::from_loader(cache, move || {
            let records = assets::read_registry_for_protocol(
                assets::pinned_block_registry_bytes(),
                assets::active_content_registry_protocol(),
            )
            .unwrap_or_else(|error| {
                eprintln!("block name registry unavailable: {error}");
                Box::default()
            });
            let mut names: HashMap<Box<str>, Vec<u32>> = HashMap::new();
            for record in records {
                let id = match mode {
                    NetworkIdMode::Sequential => record.sequential_id,
                    NetworkIdMode::Hashed => record.network_hash,
                };
                names.entry(record.name).or_default().push(id);
            }
            names
                .into_iter()
                .map(|(name, ids)| (name, ids.into()))
                .collect()
        })
    }

    /// Starts one background registry load and shares its result without waiting for it.
    fn from_loader(
        cache: &OnceLock<SharedNameIndex>,
        load: impl FnOnce() -> NameIndex + Send + 'static,
    ) -> Self {
        let base = cache.get_or_init(|| {
            let base = Arc::new(OnceLock::new());
            let pending = Arc::clone(&base);
            std::thread::spawn(move || {
                let _ = pending.set(load());
            });
            base
        });
        Self {
            base: Arc::clone(base),
            custom: HashMap::new(),
            revision: 0,
        }
    }

    /// Borrows custom states first, then states from the shared pinned index.
    fn ids(&self, name: &str) -> &[u32] {
        self.custom
            .get(name)
            .map(Vec::as_slice)
            .or_else(|| self.base.get()?.get(name).map(AsRef::as_ref))
            .unwrap_or(&[])
    }

    /// Tracks changes that require consumers to resolve requested names again.
    fn revision(&self) -> u64 {
        self.revision
            .wrapping_mul(2)
            .wrapping_add(u64::from(self.base.get().is_some()))
    }
}

impl WorldAuthority {
    /// Resolves a block identifier to the active session's internal store IDs.
    pub fn block_ids_for_name(&self, name: &str) -> &[u32] {
        self.block_names.ids(name)
    }

    /// Changes when custom names are replaced or the background pinned index becomes ready.
    pub fn block_name_revision(&self) -> u64 {
        self.block_names.revision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_block_name_index_loads_off_join_thread_and_refreshes_consumers() {
        let cache = OnceLock::new();
        let joining_thread = std::thread::current().id();
        let (release, pending) = std::sync::mpsc::channel();
        let mut names = BlockNames::from_loader(&cache, move || {
            assert_ne!(
                std::thread::current().id(),
                joining_thread,
                "registry decoding ran on the join thread"
            );
            pending.recv().unwrap();
            HashMap::from([("example:pinned".into(), Box::from([7, 8]))])
        });
        names.custom.insert("example:custom".into(), vec![12]);
        let before = names.revision();
        assert_eq!(names.ids("example:custom"), [12]);
        assert!(names.ids("example:pinned").is_empty());
        let shared = BlockNames::from_loader(&cache, || panic!("registry loaded twice"));
        release.send(()).unwrap();
        test_time::eventually("background block name index", || {
            !names.ids("example:pinned").is_empty()
        });
        assert_eq!(names.ids("example:pinned"), [7, 8]);
        assert_eq!(shared.ids("example:pinned"), [7, 8]);
        assert_ne!(names.revision(), before);
    }
}
