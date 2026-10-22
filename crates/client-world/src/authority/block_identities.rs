use std::{collections::HashMap, sync::OnceLock};

use super::*;

/// Immutable custom identities and names prepared for one admitted palette.
#[derive(Clone, Debug)]
pub struct PreparedBlockIdentities {
    mode: NetworkIdMode,
    custom_ids: std::ops::Range<u32>,
    identities: Arc<HashMap<u32, u32>>,
    names: Arc<HashMap<Arc<str>, Vec<u32>>>,
    skipped: usize,
}

impl PreparedBlockIdentities {
    /// Resolves canonical state hashes once, retaining the existing palette and name semantics.
    pub fn new(
        definitions: &CustomBlocks,
        mode: NetworkIdMode,
        custom_ids: std::ops::Range<u32>,
    ) -> Self {
        let mut identities = HashMap::new();
        let mut names: HashMap<Arc<str>, Vec<u32>> = HashMap::new();
        let mut next = custom_ids.start;
        let mut skipped = 0_usize;
        for (index, block) in definitions.blocks.iter().enumerate() {
            let first = next;
            if mode == NetworkIdMode::Sequential {
                let Some(end) = next.checked_add(block.state_count) else {
                    skipped = skipped.saturating_add(definitions.blocks.len() - index);
                    break;
                };
                next = end;
                if end > custom_ids.end {
                    skipped = skipped.saturating_add(1);
                    continue;
                }
                names
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
                let internal = match mode {
                    NetworkIdMode::Sequential => first + offset as u32,
                    NetworkIdMode::Hashed => state.hash,
                };
                identities.entry(state.hash).or_insert(internal);
                if mode == NetworkIdMode::Hashed {
                    names
                        .entry(Arc::clone(&block.name))
                        .or_default()
                        .push(internal);
                }
            }
        }
        Self {
            mode,
            custom_ids,
            identities: Arc::new(identities),
            names: Arc::new(names),
            skipped,
        }
    }
}

impl WorldAuthority {
    /// Prepares and installs identities for callers that already own the session mutation.
    pub fn set_custom_block_identities(&mut self, definitions: &CustomBlocks) {
        let prepared = PreparedBlockIdentities::new(
            definitions,
            self.network_id_mode,
            self.custom_block_ids.clone(),
        );
        self.install_custom_block_identities(prepared);
    }

    /// Installs an immutable identity view without enumerating custom states on the commit path.
    #[expect(
        clippy::print_stderr,
        reason = "SESSION_CUSTOM_BLOCK_IDENTITIES is a stderr marker read during live sessions"
    )]
    pub fn install_custom_block_identities(&mut self, prepared: PreparedBlockIdentities) {
        assert_eq!(prepared.mode, self.network_id_mode);
        assert_eq!(prepared.custom_ids, self.custom_block_ids);
        self.block_names.revision = self.block_names.revision.wrapping_add(1);
        self.block_names.custom = prepared.names;
        self.custom_block_identities = prepared.identities;
        eprintln!(
            "SESSION_CUSTOM_BLOCK_IDENTITIES session={} mode={:?} states={} skipped_definitions={}",
            self.actor_session_id,
            self.network_id_mode,
            self.custom_block_identities.len(),
            prepared.skipped,
        );
    }
}

type NameIndex = HashMap<Box<str>, Box<[u32]>>;
type SharedNameIndex = Arc<OnceLock<NameIndex>>;

/// Names resolve to internal store identities, including all admitted custom states.
#[derive(Debug)]
pub(super) struct BlockNames {
    base: SharedNameIndex,
    custom: Arc<HashMap<Arc<str>, Vec<u32>>>,
    revision: u64,
}

impl BlockNames {
    /// Removes names from the previous custom range before a new palette is admitted.
    pub(super) fn clear_custom(&mut self) {
        self.custom = Arc::default();
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
        Self::from_loader(
            cache,
            move || {
                let records = assets::read_registry_for_protocol(
                    assets::pinned_block_registry_bytes(),
                    assets::active_content_registry_protocol(),
                )
                .unwrap_or_else(|error| {
                    tracing::warn!("block name registry unavailable: {error}");
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
            },
            |work| {
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::spawn(work);
                #[cfg(target_arch = "wasm32")]
                crate::browser_jobs::registry(mode, work);
            },
        )
    }

    /// Starts one background registry load and shares its result without waiting for it.
    fn from_loader(
        cache: &OnceLock<SharedNameIndex>,
        load: impl FnOnce() -> NameIndex + Send + 'static,
        submit: impl FnOnce(Box<dyn FnOnce() + Send>),
    ) -> Self {
        let base = cache.get_or_init(|| {
            let base = Arc::new(OnceLock::new());
            let pending = Arc::clone(&base);
            submit(Box::new(move || {
                let _ = pending.set(load());
            }));
            base
        });
        Self {
            base: Arc::clone(base),
            custom: Arc::default(),
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

    /// What vanilla's adventure block-permission check sees in the hand for one edit of
    /// the block with store id `block`: an empty hand, or whether the held item's
    /// CanPlaceOn or CanDestroy `list` names that block. A malformed list names nothing.
    pub fn held_edit_item(
        &self,
        item: &protocol::VerifiedNetworkItemStack,
        list: protocol::ItemBlockList,
        block: u32,
    ) -> crate::game_mode_capabilities::HeldEditItem {
        use crate::game_mode_capabilities::HeldEditItem;
        if item.is_empty() {
            return HeldEditItem::EmptyHand;
        }
        let lists_block = protocol::item_block_list(item.extra_data(), list).is_some_and(|names| {
            names
                .iter()
                .any(|name| self.block_ids_for_name(name).contains(&block))
        });
        HeldEditItem::Item { lists_block }
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
        let mut names = BlockNames::from_loader(
            &cache,
            move || {
                assert_ne!(
                    std::thread::current().id(),
                    joining_thread,
                    "registry decoding ran on the join thread"
                );
                pending.recv().unwrap();
                HashMap::from([("example:pinned".into(), Box::from([7, 8]))])
            },
            |work| {
                std::thread::spawn(work);
            },
        );
        Arc::make_mut(&mut names.custom).insert("example:custom".into(), vec![12]);
        let before = names.revision();
        assert_eq!(names.ids("example:custom"), [12]);
        assert!(names.ids("example:pinned").is_empty());
        let shared = BlockNames::from_loader(
            &cache,
            || panic!("registry loaded twice"),
            |_| panic!("registry scheduled twice"),
        );
        release.send(()).unwrap();
        test_time::eventually("background block name index", || {
            !names.ids("example:pinned").is_empty()
        });
        assert_eq!(names.ids("example:pinned"), [7, 8]);
        assert_eq!(shared.ids("example:pinned"), [7, 8]);
        assert_ne!(names.revision(), before);
    }

    #[test]
    fn browser_block_names_wait_for_the_host_job_and_publish_once() {
        let cache = OnceLock::new();
        let names = BlockNames::from_loader(
            &cache,
            || HashMap::from([("example:deferred".into(), Box::from([17]))]),
            |work| crate::browser_jobs::registry(NetworkIdMode::Sequential, work),
        );
        let before = names.revision();
        assert!(names.ids("example:deferred").is_empty());
        let shared = BlockNames::from_loader(
            &cache,
            || panic!("registry loaded twice"),
            |_| panic!("registry scheduled twice"),
        );
        assert!(crate::browser_jobs::run_one());
        assert_eq!(names.ids("example:deferred"), [17]);
        assert_eq!(shared.ids("example:deferred"), [17]);
        assert_ne!(names.revision(), before);
        assert!(!crate::browser_jobs::run_one());
    }

    /// Supplies a minimal authority with the same negotiated palette mode as its prepared data.
    fn authority(mode: NetworkIdMode) -> WorldAuthority {
        WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: mode == NetworkIdMode::Hashed,
            },
            Arc::new(RuntimeAssets::diagnostic()),
            None,
            [0.0; 3],
            None,
        )
    }

    #[test]
    fn prepared_identities_install_without_rebuilding_names_or_state_maps() {
        use world::BlockIds;
        let definitions =
            CustomBlocks::from_definitions([("example:prepared", b"\x0a\x00\x00".as_slice())]);
        let hash = definitions.blocks[0].hashed_states()[0].hash;
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let ids = if mode == NetworkIdMode::Sequential {
                10..11
            } else {
                0..0
            };
            let prepared = PreparedBlockIdentities::new(&definitions, mode, ids.clone());
            let names = Arc::clone(&prepared.names);
            let identities = Arc::clone(&prepared.identities);
            let mut world = authority(mode);
            world.set_custom_block_ids(ids.clone());
            let before = world.block_name_revision();
            world.install_custom_block_identities(prepared);
            assert!(Arc::ptr_eq(&identities, &world.custom_block_identities));
            assert!(Arc::ptr_eq(&names, &world.block_names.custom));
            let id = if mode == NetworkIdMode::Sequential {
                10
            } else {
                hash
            };
            assert_eq!(world.block_ids_for_name("example:prepared"), [id]);
            assert_eq!(world.decode_ids(0).resolve(id), id);
            assert_ne!(world.block_name_revision(), before);
            world.install_custom_block_identities(PreparedBlockIdentities::new(
                &CustomBlocks::default(),
                mode,
                ids,
            ));
            assert!(world.block_ids_for_name("example:prepared").is_empty());
            assert!(world.custom_block_identities.is_empty());
            assert_eq!(identities.get(&hash), Some(&id));
            assert_eq!(names.get("example:prepared").unwrap(), &[id]);
        }
    }

    /// A held stack whose user data carries `can_destroy` and an empty CanPlaceOn list.
    fn stack_listing(can_destroy: &[&str]) -> protocol::VerifiedNetworkItemStack {
        use sha2::Digest as _;
        let mut extra = 0_i16.to_le_bytes().to_vec();
        extra.extend_from_slice(&0_i32.to_le_bytes());
        extra.extend_from_slice(&(can_destroy.len() as i32).to_le_bytes());
        for name in can_destroy {
            extra.extend_from_slice(&(name.len() as u16).to_le_bytes());
            extra.extend_from_slice(name.as_bytes());
        }
        let extra_data: Arc<[u8]> = Arc::from(extra);
        let stack = protocol::NetworkItemStack {
            network_id: 5,
            stack_network_id: 41,
            count: 1,
            nbt_digest: sha2::Sha256::digest(&extra_data).into(),
            extra_data,
            ..protocol::NetworkItemStack::empty()
        };
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap()
    }

    #[test]
    fn held_edit_item_reads_whether_the_item_list_names_the_block() {
        use crate::game_mode_capabilities::HeldEditItem;
        let definitions =
            CustomBlocks::from_definitions([("example:listed", b"\x0a\x00\x00".as_slice())]);
        let mut world = authority(NetworkIdMode::Sequential);
        world.set_custom_block_ids(10..11);
        world.install_custom_block_identities(PreparedBlockIdentities::new(
            &definitions,
            NetworkIdMode::Sequential,
            10..11,
        ));
        let destroy = protocol::ItemBlockList::CanDestroy;
        assert_eq!(
            world.held_edit_item(&stack_listing(&["example:listed"]), destroy, 10),
            HeldEditItem::Item { lists_block: true }
        );
        assert_eq!(
            world.held_edit_item(&stack_listing(&["example:listed"]), destroy, 11),
            HeldEditItem::Item { lists_block: false }
        );
        assert_eq!(
            world.held_edit_item(&stack_listing(&[]), destroy, 10),
            HeldEditItem::Item { lists_block: false }
        );
        assert_eq!(
            world.held_edit_item(
                &stack_listing(&["example:listed"]),
                protocol::ItemBlockList::CanPlaceOn,
                10
            ),
            HeldEditItem::Item { lists_block: false }
        );
        let empty = protocol::VerifiedNetworkItemStack::try_new(
            protocol::NetworkItemStack::empty(),
            protocol::NetworkItemStack::empty().nbt_digest,
        )
        .unwrap();
        assert_eq!(
            world.held_edit_item(&empty, destroy, 10),
            HeldEditItem::EmptyHand
        );
    }
}
