//! Ordered, bounded server cracking authority. Server values are not a clock.

use super::*;

/// A client resource budget, not a gameplay limit.
pub const MAX_ACTIVE_BLOCK_CRACKS: usize = 1_024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlockCrackStatus {
    pub active: usize,
    pub server_value_sum: u64,
    pub consumed: u64,
    pub orphan_updates: u64,
    pub capacity_rejections: u64,
    pub unsupported_values: u64,
    pub unsupported_targets: u64,
    pub retired_targets: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActiveBlockCrack {
    pub position: [i32; 3],
    pub start_sequence: u64,
    /// The validated server value, without inferred progress or expiry.
    pub server_value: u16,
    pub layers: [Option<u32>; world::MAX_STORAGE_COUNT],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockCrackSnapshot {
    pub session_id: u64,
    pub dimension: i32,
    /// Changes on every committed dimension replacement, including same-ID replacements.
    pub dimension_sequence: Option<u64>,
    pub entries: Vec<ActiveBlockCrack>,
    pub status: BlockCrackStatus,
}

#[derive(Default)]
pub(super) struct BlockCracks {
    columns: BTreeMap<ChunkKey, BTreeMap<[i32; 3], ActiveBlockCrack>>,
    status: BlockCrackStatus,
    dimension_sequence: Option<u64>,
}

impl WorldStream {
    pub fn block_crack_snapshot(&self) -> BlockCrackSnapshot {
        let entries = self
            .block_cracks
            .columns
            .values()
            .flat_map(|column| column.values().cloned())
            .collect::<Vec<_>>();
        BlockCrackSnapshot {
            session_id: self.authority.actor_session_id(),
            dimension: self.authority.current_dimension(),
            dimension_sequence: self.block_cracks.dimension_sequence,
            status: BlockCrackStatus {
                server_value_sum: entries
                    .iter()
                    .map(|entry| u64::from(entry.server_value))
                    .sum(),
                ..self.block_cracks.status
            },
            entries,
        }
    }

    fn crack_column(&self, position: [i32; 3]) -> ChunkKey {
        ChunkKey::new(
            self.authority.current_dimension(),
            position[0].div_euclid(16),
            position[2].div_euclid(16),
        )
    }

    fn crack_layers(
        &self,
        column: ChunkKey,
        position: [i32; 3],
    ) -> Option<[Option<u32>; world::MAX_STORAGE_COUNT]> {
        let key = SubChunkKey::from_chunk(column, position[1].div_euclid(16));
        if !self.authority.terrain().is_sub_chunk_loaded(key) {
            return None;
        }
        let chunk = self.authority.terrain().sub_chunk(key)?;
        let [x, y, z] =
            position.map(|value| u8::try_from(value.rem_euclid(16)).expect("local coordinate"));
        let runtime_id = chunk.runtime_id(0, x, y, z)?;
        let block = self
            .authority
            .runtime_assets()
            .resolve(self.authority.network_id_mode(), runtime_id);
        if !block.is_known() || block.flags().contains(assets::BlockFlags::AIR) {
            return None;
        }
        Some(std::array::from_fn(|layer| {
            chunk.runtime_id(layer, x, y, z)
        }))
    }

    pub(super) fn consume_block_crack(&mut self, sequence: u64, event: BlockCrackEvent) {
        self.block_cracks.status.consumed = self.block_cracks.status.consumed.saturating_add(1);
        let column = self.crack_column(event.position);
        match event.action {
            client_world::ingestion::BlockCrackAction::Stop => {
                if let Some(entries) = self.block_cracks.columns.get_mut(&column) {
                    if entries.remove(&event.position).is_some() {
                        self.block_cracks.status.active -= 1;
                    }
                    if entries.is_empty() {
                        self.block_cracks.columns.remove(&column);
                    }
                }
            }
            client_world::ingestion::BlockCrackAction::Start { progress_per_tick } => {
                let Some(layers) = self.crack_layers(column, event.position) else {
                    self.block_cracks.status.unsupported_targets = self
                        .block_cracks
                        .status
                        .unsupported_targets
                        .saturating_add(1);
                    return;
                };
                let exists = self
                    .block_cracks
                    .columns
                    .get(&column)
                    .is_some_and(|entries| entries.contains_key(&event.position));
                if !exists && self.block_cracks.status.active >= MAX_ACTIVE_BLOCK_CRACKS {
                    self.block_cracks.status.capacity_rejections = self
                        .block_cracks
                        .status
                        .capacity_rejections
                        .saturating_add(1);
                    return;
                }
                self.block_cracks.columns.entry(column).or_default().insert(
                    event.position,
                    ActiveBlockCrack {
                        position: event.position,
                        start_sequence: sequence,
                        server_value: progress_per_tick,
                        layers,
                    },
                );
                if !exists {
                    self.block_cracks.status.active += 1;
                }
            }
            client_world::ingestion::BlockCrackAction::UpdateSpeed { progress_per_tick } => {
                if let Some(entry) = self
                    .block_cracks
                    .columns
                    .get_mut(&column)
                    .and_then(|entries| entries.get_mut(&event.position))
                {
                    entry.server_value = progress_per_tick;
                } else {
                    self.block_cracks.status.orphan_updates =
                        self.block_cracks.status.orphan_updates.saturating_add(1);
                }
            }
        }
    }

    /// Called at each successful mutation, before the next ordered event can commit.
    pub(super) fn reconcile_block_crack_column(&mut self, column: ChunkKey) {
        let Some(mut entries) = self.block_cracks.columns.remove(&column) else {
            return;
        };
        let before = entries.len();
        entries
            .retain(|position, entry| self.crack_layers(column, *position) == Some(entry.layers));
        self.retire_block_crack_count(before - entries.len());
        if !entries.is_empty() {
            self.block_cracks.columns.insert(column, entries);
        }
    }

    fn retire_block_crack_count(&mut self, count: usize) {
        self.block_cracks.status.active -= count;
        self.block_cracks.status.retired_targets = self
            .block_cracks
            .status
            .retired_targets
            .saturating_add(count as u64);
    }

    pub(super) fn evict_block_crack_column(&mut self, column: ChunkKey) {
        if let Some(entries) = self.block_cracks.columns.remove(&column) {
            self.retire_block_crack_count(entries.len());
        }
    }

    pub(super) fn replace_block_crack_dimension(&mut self, sequence: u64) {
        let count = self.block_cracks.status.active;
        self.block_cracks.columns.clear();
        self.retire_block_crack_count(count);
        self.block_cracks.dimension_sequence = Some(sequence);
    }
}
