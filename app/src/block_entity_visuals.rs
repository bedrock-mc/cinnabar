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
    pub const fn new(sequential_id: u32, network_hash: u32, visual_kind: VisualKind) -> Self {
        Self {
            sequential_id,
            network_hash: Some(network_hash),
            visual_kind,
            known: true,
        }
    }

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
        let mut identity = Self::new(
            sequential_id,
            network_hash.unwrap_or(u32::MAX),
            resolved.kind(),
        );
        identity.network_hash = network_hash;
        identity.known = resolved.is_known();
        identity
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
}

impl BlockEntityVisualDiagnostics {
    pub fn upsert(&mut self, key: BlockEntityKey, route: BlockEntityVisualRoute) {
        debug_assert_ne!(route.route_digest(), [0; 32]);
        self.routes.insert(key, route);
    }

    pub fn remove(&mut self, key: BlockEntityKey) {
        self.routes.remove(&key);
    }

    pub fn remove_sub_chunk(&mut self, key: SubChunkKey) {
        self.routes.retain(|entity, _| entity.sub_chunk() != key);
    }

    pub fn remove_chunk(&mut self, key: ChunkKey) {
        self.routes.retain(|entity, _| entity.chunk() != key);
    }

    pub fn clear(&mut self) {
        self.routes.clear();
    }

    #[must_use]
    pub fn counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for route in self.routes.values() {
            let index = match route {
                BlockEntityVisualRoute::ExistingBlockState { .. } => 0,
                BlockEntityVisualRoute::LogicalNoAdditionalDraw { .. } => 1,
                BlockEntityVisualRoute::Deferred { .. } => 2,
                BlockEntityVisualRoute::Unknown { .. } => 3,
            };
            counts[index] += 1;
        }
        counts
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
    digest.update(backing.sequential_id.to_le_bytes());
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
    if !backing.known {
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
