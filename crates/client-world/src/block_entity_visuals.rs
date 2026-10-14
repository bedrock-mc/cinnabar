use std::collections::BTreeMap;

use assets::{
    NetworkIdMode, RuntimeAssets, VisualKind,
    facts::{BlockEntityBacking, BlockEntityDiagnostic, BlockEntityFacts},
};
use sha2::{Digest, Sha256};
use world::{BlockEntityKey, BlockEntityNbt, ChunkKey, RootByteCandidate, SubChunkKey};

const ROUTE_DIGEST_DOMAIN: &[u8] = b"rust-mcbe:block-entity-visual-route:v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackingBlockIdentity {
    sequential_id: u32,
    network_hash: Option<u32>,
    visual_kind: VisualKind,
    full_cube_model: bool,
    known: bool,
    fact: Option<BlockEntityBacking<'static>>,
}

impl BackingBlockIdentity {
    /// Resolves the state identity and validates model geometry used by static barrel routes.
    #[must_use]
    pub fn from_runtime(value: u32, mode: NetworkIdMode, assets: &RuntimeAssets) -> Self {
        let resolved = assets.resolve(mode, value);
        let fact = BlockEntityFacts::pinned().for_runtime(value, mode, assets);
        let (sequential_id, network_hash) = match mode {
            NetworkIdMode::Sequential => (value, fact.map(|fact| fact.network_hash)),
            NetworkIdMode::Hashed => (
                assets.sequential_id_for_hash(value).unwrap_or(u32::MAX),
                Some(value),
            ),
        };
        Self {
            sequential_id,
            network_hash,
            visual_kind: resolved.kind(),
            full_cube_model: assets::gui_item::full_cube_model_quads(
                assets,
                assets::BlockVisualId(sequential_id),
            )
            .is_some(),
            known: resolved.is_known(),
            fact,
        }
    }
}

#[derive(Debug, Default)]
pub struct BlockEntityVisualDiagnostics {
    routes: BTreeMap<BlockEntityKey, BlockEntityVisualRoute>,
    counts: [usize; 4], // maintained on every mutation; read every frame
}

impl BlockEntityVisualDiagnostics {
    pub fn upsert(&mut self, key: BlockEntityKey, route: BlockEntityVisualRoute) {
        debug_assert_ne!(route.route_digest(), [0; 32]);
        if let Some(previous) = self.routes.insert(key, route) {
            self.counts[previous.count_index()] -= 1;
        }
        self.counts[route.count_index()] += 1;
    }

    pub fn remove(&mut self, key: BlockEntityKey) {
        if let Some(previous) = self.routes.remove(&key) {
            self.counts[previous.count_index()] -= 1;
        }
    }

    pub fn remove_sub_chunk(&mut self, key: SubChunkKey) {
        self.retain(|entity| entity.sub_chunk() != key);
    }

    pub fn remove_chunk(&mut self, key: ChunkKey) {
        self.retain(|entity| entity.chunk() != key);
    }

    /// Removes visual routes for a retired batch of columns.
    pub fn remove_chunks(&mut self, keys: &std::collections::BTreeSet<ChunkKey>) {
        self.retain(|entity| !keys.contains(&entity.chunk()));
    }

    fn retain(&mut self, keep: impl Fn(&BlockEntityKey) -> bool) {
        let counts = &mut self.counts;
        self.routes.retain(|entity, route| {
            let kept = keep(entity);
            if !kept {
                counts[route.count_index()] -= 1;
            }
            kept
        });
    }

    pub fn clear(&mut self) {
        self.routes.clear();
        self.counts = [0; 4];
    }

    /// Existing-state, logical, deferred and unknown route counts.
    #[must_use]
    pub const fn counts(&self) -> [usize; 4] {
        self.counts
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockEntityVisualRoute {
    ExistingBlockState {
        route_digest: [u8; 32],
        additional_refs: u64,
    },
    LogicalNoAdditionalDraw {
        route_digest: [u8; 32],
        additional_refs: u64,
    },
    Deferred {
        route_digest: [u8; 32],
    },
    Unknown {
        route_digest: [u8; 32],
    },
}

impl BlockEntityVisualRoute {
    const fn count_index(&self) -> usize {
        match self {
            Self::ExistingBlockState { .. } => 0,
            Self::LogicalNoAdditionalDraw { .. } => 1,
            Self::Deferred { .. } => 2,
            Self::Unknown { .. } => 3,
        }
    }

    #[must_use]
    pub const fn route_digest(&self) -> [u8; 32] {
        match self {
            Self::ExistingBlockState { route_digest, .. }
            | Self::LogicalNoAdditionalDraw { route_digest, .. }
            | Self::Deferred { route_digest }
            | Self::Unknown { route_digest } => *route_digest,
        }
    }
}

#[must_use]
pub fn adjudicate_block_entity_visual(
    source: &BlockEntityNbt,
    backing: BackingBlockIdentity,
) -> BlockEntityVisualRoute {
    let outcome = backing
        .fact
        .filter(|fact| {
            backing.known
                && fact
                    .entity
                    .geometry
                    .accepts(backing.visual_kind, backing.full_cube_model)
                && match source.id() {
                    Some(id) => !fact.entity.idless && id == fact.entity.nbt_id,
                    None => {
                        fact.entity.idless
                            && matches!(source.note_candidate(), RootByteCandidate::Value(0..=24))
                            && matches!(source.powered_candidate(), RootByteCandidate::Value(0..=1))
                    }
                }
        })
        .map_or(RouteOutcome::Unknown, |fact| match fact.entity.diagnostic {
            BlockEntityDiagnostic::ExistingBlockState => RouteOutcome::Static,
            BlockEntityDiagnostic::Logical => RouteOutcome::Logical,
            BlockEntityDiagnostic::Deferred => RouteOutcome::Deferred,
        });
    let route_digest = route_digest(outcome, source, backing);
    match outcome {
        RouteOutcome::Static => BlockEntityVisualRoute::ExistingBlockState {
            route_digest,
            additional_refs: 0,
        },
        RouteOutcome::Logical => BlockEntityVisualRoute::LogicalNoAdditionalDraw {
            route_digest,
            additional_refs: 0,
        },
        RouteOutcome::Deferred => BlockEntityVisualRoute::Deferred { route_digest },
        RouteOutcome::Unknown => BlockEntityVisualRoute::Unknown { route_digest },
    }
}

/// Binds the outcome to the retained NBT identity and admitted backing state.
fn route_digest(
    outcome: RouteOutcome,
    source: &BlockEntityNbt,
    backing: BackingBlockIdentity,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(ROUTE_DIGEST_DOMAIN);
    digest.update([outcome as u8]);
    match source.id() {
        Some(id) => {
            digest.update([1]);
            digest.update((id.len() as u64).to_le_bytes());
            digest.update(id.as_bytes());
        }
        None => {
            digest.update([0]);
            update_candidate_digest(&mut digest, source.note_candidate());
            update_candidate_digest(&mut digest, source.powered_candidate());
        }
    }
    digest.update([u8::from(backing.known), backing.visual_kind as u8]);
    digest.update(backing.sequential_id.to_le_bytes());
    if !backing.known {
        match backing.network_hash {
            Some(network_hash) => {
                digest.update([1]);
                digest.update(network_hash.to_le_bytes());
            }
            None => digest.update([0]),
        }
    }
    digest.finalize().into()
}

/// Includes absent and malformed idless fields in the diagnostic identity.
fn update_candidate_digest(digest: &mut Sha256, candidate: RootByteCandidate) {
    match candidate {
        RootByteCandidate::Absent => digest.update([0, 0]),
        RootByteCandidate::Value(value) => digest.update([1, value]),
        RootByteCandidate::Invalid => digest.update([2, 0]),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum RouteOutcome {
    Static = 1,
    Logical = 2,
    Deferred = 3,
    Unknown = 4,
}

#[cfg(test)]
mod adjudication_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Incremental counts must match a full recount after any mix of mutations.
    #[test]
    fn maintained_route_counts_match_a_recount() {
        let key = |x, y| BlockEntityKey::new(0, x, y, 0);
        let deferred = BlockEntityVisualRoute::Deferred {
            route_digest: [1; 32],
        };
        let unknown = BlockEntityVisualRoute::Unknown {
            route_digest: [2; 32],
        };
        let mut diagnostics = BlockEntityVisualDiagnostics::default();
        for x in 0..40 {
            diagnostics.upsert(key(x, 1), deferred);
            diagnostics.upsert(key(x, 20), unknown);
        }
        diagnostics.upsert(key(0, 1), unknown);
        diagnostics.remove(key(1, 1));
        diagnostics.remove(key(1, 1));
        diagnostics.remove_sub_chunk(key(2, 20).sub_chunk());
        diagnostics.remove_chunk(key(3, 1).chunk());
        let mut recount = [0; 4];
        for route in diagnostics.routes.values() {
            recount[route.count_index()] += 1;
        }
        assert_eq!(diagnostics.counts(), recount);
        diagnostics.clear();
        assert_eq!(diagnostics.counts(), [0; 4]);
    }

    /// Run: `cargo test -p client-world --lib route_count_cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark"]
    fn route_count_cost_with_4096_block_entities() {
        let mut diagnostics = BlockEntityVisualDiagnostics::default();
        for x in 0..4_096 {
            diagnostics.upsert(
                BlockEntityKey::new(0, x, 64, 0),
                BlockEntityVisualRoute::Deferred {
                    route_digest: [1; 32],
                },
            );
        }
        let calls = 600;
        let started = std::time::Instant::now();
        for _ in 0..calls {
            let mut recount = [0_usize; 4];
            for route in diagnostics.routes.values() {
                recount[route.count_index()] += 1;
            }
            std::hint::black_box(recount);
        }
        let old = started.elapsed() / calls;
        let started = std::time::Instant::now();
        for _ in 0..calls {
            std::hint::black_box(diagnostics.counts());
        }
        let new = started.elapsed() / calls;
        eprintln!(
            "FRAME_COST block_entity_route_counts_4096: old={:.4}ms new={:.4}ms per stats() call",
            old.as_secs_f64() * 1e3,
            new.as_secs_f64() * 1e3
        );
    }
}
