use std::collections::BTreeMap;

use assets::{NetworkIdMode, RuntimeAssets, VisualKind};
use sha2::{Digest, Sha256};
use world::{BlockEntityKey, BlockEntityNbt, ChunkKey, RootByteCandidate, SubChunkKey};

const ROUTE_DIGEST_DOMAIN: &[u8] = b"rust-mcbe:block-entity-visual-route:v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackingBlockIdentity {
    sequential_id: u32,
    network_hash: Option<u32>,
    visual_kind: VisualKind,
    known: bool,
}

impl BackingBlockIdentity {
    #[must_use]
    pub fn from_runtime(value: u32, mode: NetworkIdMode, assets: &RuntimeAssets) -> Self {
        let resolved = assets.resolve(mode, value);
        let (sequential_id, network_hash) = match mode {
            NetworkIdMode::Sequential => (value, expected_network_hash(value)),
            NetworkIdMode::Hashed => (
                assets.sequential_id_for_hash(value).unwrap_or(u32::MAX),
                Some(value),
            ),
        };
        Self {
            sequential_id,
            network_hash,
            visual_kind: resolved.kind(),
            known: resolved.is_known(),
        }
    }

    fn matches(self, expected: StaticSource) -> bool {
        self.known
            && self.visual_kind == VisualKind::Cube
            && static_backing(self.sequential_id).is_some_and(|record| {
                record.source == expected && Some(record.hash) == self.network_hash
            })
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
    let outcome = match source.id() {
        Some("Barrel") => static_outcome(backing, StaticSource::Barrel),
        Some("BlastFurnace") => static_outcome(backing, StaticSource::BlastFurnace),
        Some("Furnace") => static_outcome(backing, StaticSource::Furnace),
        Some("Smoker") => static_outcome(backing, StaticSource::Smoker),
        Some("Jukebox") => {
            if backing.matches(StaticSource::Jukebox) {
                RouteOutcome::Logical
            } else {
                RouteOutcome::Unknown
            }
        }
        Some(id) if REVIEWED_DEFERRED_IDS.contains(&id) => {
            if deferred_backing_matches(id, backing) {
                RouteOutcome::Deferred
            } else {
                RouteOutcome::Unknown
            }
        }
        Some(_) => RouteOutcome::Unknown,
        None => {
            if backing.matches(StaticSource::Note)
                && matches!(source.note_candidate(), RootByteCandidate::Value(0..=24))
                && matches!(source.powered_candidate(), RootByteCandidate::Value(0..=1))
            {
                RouteOutcome::Logical
            } else {
                RouteOutcome::Unknown
            }
        }
    };
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

fn static_outcome(backing: BackingBlockIdentity, expected: StaticSource) -> RouteOutcome {
    if backing.matches(expected) {
        RouteOutcome::Static
    } else {
        RouteOutcome::Unknown
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaticSource {
    Barrel,
    BlastFurnace,
    Furnace,
    Smoker,
    Jukebox,
    Note,
}

#[derive(Debug, Clone, Copy)]
struct StaticBacking {
    sequential_id: u32,
    hash: u32,
    source: StaticSource,
}

fn static_backing(sequential_id: u32) -> Option<&'static StaticBacking> {
    STATIC_BACKINGS
        .binary_search_by_key(&sequential_id, |record| record.sequential_id)
        .ok()
        .map(|index| &STATIC_BACKINGS[index])
}

fn expected_network_hash(sequential_id: u32) -> Option<u32> {
    static_backing(sequential_id).map(|record| record.hash)
}

fn deferred_backing_matches(id: &str, backing: BackingBlockIdentity) -> bool {
    // Entity-drawn blocks compile to `Invisible` and the enchanting-table base to `Model`;
    // the placeholder `Diagnostic` route stays accepted for carriers built before that.
    let expected_visual_kinds: &[VisualKind] = match id {
        "Beacon" => &[VisualKind::Cube],
        "Sign" => &[VisualKind::Model],
        "Banner" | "Bed" | "Chest" | "CopperGolemStatue" | "DecoratedPot" | "EnderChest"
        | "GlowItemFrame" | "ItemFrame" | "Lectern" | "Skull" => {
            &[VisualKind::Diagnostic, VisualKind::Invisible]
        }
        "EnchantTable" => &[VisualKind::Diagnostic, VisualKind::Model],
        _ => &[VisualKind::Diagnostic],
    };
    if !backing.known || !expected_visual_kinds.contains(&backing.visual_kind) {
        return false;
    }
    let sequential_id = backing.sequential_id;
    match id {
        "Banner" => matches!(sequential_id, 10_321..=10_326 | 13_571..=13_586),
        "Beacon" => sequential_id == 846,
        "Bed" => matches!(sequential_id, 13_095..=13_110),
        "BrewingStand" => matches!(sequential_id, 15_128..=15_135),
        "Campfire" => matches!(sequential_id, 10_421..=10_428 | 15_923..=15_930),
        "Chest" => matches!(sequential_id, 14_039..=14_042),
        "CopperGolemStatue" => matches!(
            sequential_id,
            2_648..=2_651
                | 6_357..=6_360
                | 6_854..=6_857
                | 7_865..=7_868
                | 8_544..=8_547
                | 12_151..=12_154
                | 15_022..=15_025
                | 15_918..=15_921
        ),
        "DecoratedPot" => matches!(sequential_id, 13_157..=13_160),
        "EnchantTable" => sequential_id == 13_163,
        "EnderChest" => matches!(sequential_id, 6_870..=6_873),
        "GlowItemFrame" => matches!(sequential_id, 1_047..=1_070),
        "Hopper" => matches!(sequential_id, 13_514..=13_525),
        "ItemFrame" => matches!(sequential_id, 6_477..=6_500),
        "Lectern" => matches!(sequential_id, 13_559..=13_566),
        "Sign" => matches!(
            sequential_id,
            13..=28
                | 837..=842
                | 1_992..=1_997
                | 2_018..=2_023
                | 5_393..=5_398
                | 6_438..=6_453
                | 6_883..=6_888
                | 8_510..=8_515
                | 9_120..=9_125
                | 9_209..=9_214
                | 10_237..=10_252
                | 11_064..=11_079
                | 12_171..=12_186
                | 12_620..=12_635
                | 13_126..=13_141
                | 13_336..=13_347
                | 13_849..=13_864
                | 14_513..=14_528
                | 14_533..=14_548
                | 14_691..=14_722
                | 14_941..=14_946
                | 15_347..=15_352
        ),
        "Skull" => matches!(
            sequential_id,
            33..=38
                | 5_468..=5_473
                | 9_295..=9_300
                | 10_987..=10_992
                | 11_011..=11_016
                | 13_832..=13_837
                | 14_565..=14_570
        ),
        _ => false,
    }
}

const REVIEWED_DEFERRED_IDS: [&str; 16] = [
    "Banner",
    "Beacon",
    "Bed",
    "BrewingStand",
    "Campfire",
    "Chest",
    "CopperGolemStatue",
    "DecoratedPot",
    "EnchantTable",
    "EnderChest",
    "GlowItemFrame",
    "Hopper",
    "ItemFrame",
    "Lectern",
    "Sign",
    "Skull",
];

const STATIC_BACKINGS: [StaticBacking; 38] = [
    StaticBacking {
        sequential_id: 1_936,
        hash: 166_024_317,
        source: StaticSource::Note,
    },
    StaticBacking {
        sequential_id: 2_699,
        hash: 3_435_179_109,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 2_700,
        hash: 2_950_269_998,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 2_701,
        hash: 3_568_550_727,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 2_702,
        hash: 3_132_699_916,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 7_069,
        hash: 198_111_737,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_070,
        hash: 501_043_176,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_071,
        hash: 1_071_581_843,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_072,
        hash: 4_094_588_762,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_073,
        hash: 4_152_884_613,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_074,
        hash: 1_814_814_452,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_075,
        hash: 3_437_772_462,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_076,
        hash: 1_556_349_747,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_077,
        hash: 16_275_272,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_078,
        hash: 854_928_037,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_079,
        hash: 3_097_578_042,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 7_080,
        hash: 2_870_121_023,
        source: StaticSource::Barrel,
    },
    StaticBacking {
        sequential_id: 8_516,
        hash: 1_605_519_270,
        source: StaticSource::Jukebox,
    },
    StaticBacking {
        sequential_id: 13_947,
        hash: 1_464_259_042,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 13_948,
        hash: 1_215_033_323,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 13_949,
        hash: 3_697_737_228,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 13_950,
        hash: 3_322_658_681,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 14_587,
        hash: 2_568_407_871,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 14_588,
        hash: 42_144_652,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 14_589,
        hash: 1_568_180_725,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 14_590,
        hash: 1_899_400_230,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 15_143,
        hash: 2_142_573_020,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 15_144,
        hash: 3_066_600_017,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 15_145,
        hash: 184_718_330,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 15_146,
        hash: 793_504_035,
        source: StaticSource::BlastFurnace,
    },
    StaticBacking {
        sequential_id: 15_321,
        hash: 2_080_399_355,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 15_322,
        hash: 859_357_296,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 15_323,
        hash: 4_033_512_929,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 15_324,
        hash: 3_231_615_138,
        source: StaticSource::Smoker,
    },
    StaticBacking {
        sequential_id: 15_688,
        hash: 3_463_497_305,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 15_689,
        hash: 2_478_237_434,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 15_690,
        hash: 1_875_646_683,
        source: StaticSource::Furnace,
    },
    StaticBacking {
        sequential_id: 15_691,
        hash: 4_038_254_352,
        source: StaticSource::Furnace,
    },
];

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
