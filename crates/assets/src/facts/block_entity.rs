//! Registry-bound block-entity routes and backing-state membership.

use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::OnceLock};

use crate::{BlockEntityRouteKind, NetworkIdMode, RuntimeAssets, VisualKind};

/// The generated routing table owned by the active block registry.
pub const BLOCK_ENTITY_FACTS_PATH: &str = "crates/assets/data/block-entities-v2193.json";

/// The renderer that consumes an admitted block entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEntityRenderer {
    None,
    Chest,
    EnderChest,
    Shulker,
    Skull,
    Bed,
    Chalkboard,
    Banner,
    Sign,
    EnchantTable,
    Lectern,
    Bell,
    Beacon,
    ItemFrame,
    GlowItemFrame,
    FlowerPot,
    Campfire,
    Conduit,
    DecoratedPot,
    Statue,
    EndPortal,
    EndGateway,
    MobSpawner,
}

/// A mount selected from the backing state's property shape at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEntityMount {
    None,
    Standing,
    Wall,
    Hanging,
}

/// The diagnostic outcome before any additional renderer has run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEntityDiagnostic {
    ExistingBlockState,
    Logical,
    Deferred,
}

/// Geometry required before a diagnostic route can claim its backing block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockEntityGeometry {
    Cube,
    CubeOrFullModel,
    Diagnostic,
    Model,
    ModelOrDiagnostic,
    InvisibleOrDiagnostic,
    Any,
}

impl BlockEntityGeometry {
    /// Requires the admitted visual kind, including the checked full-cube barrel model.
    pub fn accepts(self, kind: VisualKind, full_cube_model: bool) -> bool {
        match self {
            Self::Cube => kind == VisualKind::Cube,
            Self::CubeOrFullModel => kind == VisualKind::Cube || full_cube_model,
            Self::Diagnostic => kind == VisualKind::Diagnostic,
            Self::Model => kind == VisualKind::Model,
            Self::ModelOrDiagnostic => matches!(kind, VisualKind::Model | VisualKind::Diagnostic),
            Self::InvisibleOrDiagnostic => {
                matches!(kind, VisualKind::Invisible | VisualKind::Diagnostic)
            }
            Self::Any => true,
        }
    }
}

/// One NBT identity and its renderer contract, shared by all its backing states.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockEntityEntry {
    pub nbt_id: String,
    pub route: BlockEntityRouteKind,
    pub renderer: BlockEntityRenderer,
    pub diagnostic: BlockEntityDiagnostic,
    pub geometry: BlockEntityGeometry,
    #[serde(default)]
    pub idless: bool,
    #[serde(default)]
    pub provisional: bool,
    #[serde(default)]
    pub backing_blocks: Vec<String>,
}

/// One sparse generated binding; consumers use a dense numeric index.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockEntityState {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub entry: usize,
    pub mount: BlockEntityMount,
}

/// Version-bound output of registryc's block-entity pass.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BlockEntityFactsPayload {
    pub schema: u32,
    pub protocol: u32,
    pub registry_sha256: String,
    pub entries: Vec<BlockEntityEntry>,
    pub states: Vec<BlockEntityState>,
}

/// An admitted backing state's typed renderer and mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockEntityBacking<'a> {
    pub entity: &'a BlockEntityEntry,
    pub mount: BlockEntityMount,
    pub network_hash: u32,
}

/// One immutable table with dense backing-state lookups.
#[derive(Debug)]
pub struct BlockEntityFacts {
    payload: BlockEntityFactsPayload,
    states: Box<[Option<BlockEntityState>]>,
    registry_sha256: [u8; 32],
    hashes: HashMap<u32, u32>,
    blocks: HashMap<Box<str>, usize>,
}

impl BlockEntityFacts {
    /// Loads the committed table once and verifies its exact registry binding.
    pub fn pinned() -> &'static Self {
        static FACTS: OnceLock<BlockEntityFacts> = OnceLock::new();
        FACTS.get_or_init(|| {
            let bytes = include_bytes!("../../data/block-entities-v2193.json");
            Self::decode(bytes, crate::pinned_block_registry_bytes())
                .expect("valid generated block-entity facts; regenerate with registryc")
        })
    }

    /// Rejects another palette, invalid state identities, duplicate bindings or empty routes.
    pub fn decode(bytes: &[u8], registry: &[u8]) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        let payload: BlockEntityFactsPayload =
            serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let hash: [u8; 32] = Sha256::digest(registry).into();
        if payload.schema != 1
            || payload.registry_sha256 != format!("{:x}", Sha256::digest(registry))
        {
            return Err("block-entity registry binding mismatch".into());
        }
        let records = crate::read_registry_for_protocol(registry, payload.protocol)
            .map_err(|e| e.to_string())?;
        if !payload
            .entries
            .windows(2)
            .all(|p| p[0].nbt_id < p[1].nbt_id)
        {
            return Err("block-entity routes are not unique and sorted".into());
        }
        let mut states = vec![None; records.len()];
        let mut counts = vec![0; payload.entries.len()];
        for state in &payload.states {
            let record = records
                .get(state.sequential_id as usize)
                .ok_or("invalid block-entity runtime id")?;
            let entry = payload
                .entries
                .get(state.entry)
                .ok_or("invalid block-entity entry")?;
            if record.sequential_id != state.sequential_id
                || record.network_hash != state.network_hash
                || !entry
                    .backing_blocks
                    .iter()
                    .any(|name| name == record.name.as_ref())
                || states[state.sequential_id as usize]
                    .replace(*state)
                    .is_some()
            {
                return Err("invalid or duplicate block-entity backing".into());
            }
            counts[state.entry] += 1;
        }
        if payload.entries.is_empty() || counts.contains(&0) {
            return Err("block-entity route has no backing states".into());
        }
        let hashes = payload
            .states
            .iter()
            .map(|state| (state.network_hash, state.sequential_id))
            .collect();
        let mut blocks = HashMap::new();
        for (index, entry) in payload.entries.iter().enumerate() {
            for name in &entry.backing_blocks {
                blocks.entry(name.clone().into_boxed_str()).or_insert(index);
            }
        }
        Ok(Self {
            hashes,
            blocks,
            payload,
            states: states.into(),
            registry_sha256: hash,
        })
    }

    /// Resolves an NBT id once when its retained record is admitted.
    pub fn entry(&self, id: &str) -> Option<&BlockEntityEntry> {
        self.payload
            .entries
            .binary_search_by(|entry| entry.nbt_id.as_str().cmp(id))
            .ok()
            .map(|i| &self.payload.entries[i])
    }

    /// Resolves an admitted block identity without scanning the route table.
    pub fn by_block_identifier(&self, identifier: &str) -> Option<&BlockEntityEntry> {
        self.blocks
            .get(identifier)
            .map(|&index| &self.payload.entries[index])
    }

    /// Returns a backing route in one numeric array lookup.
    pub fn get(&self, sequential_id: u32) -> Option<BlockEntityBacking<'_>> {
        let state = self.states.get(sequential_id as usize)?.as_ref()?;
        Some(BlockEntityBacking {
            entity: &self.payload.entries[state.entry],
            mount: state.mount,
            network_hash: state.network_hash,
        })
    }

    /// Resolves backing identity against a checked registry, independently of world artwork.
    pub fn for_registry(
        &self,
        value: u32,
        mode: NetworkIdMode,
        registry_sha256: [u8; 32],
    ) -> Option<BlockEntityBacking<'_>> {
        if registry_sha256 != self.registry_sha256 {
            return None;
        }
        let sequential = match mode {
            NetworkIdMode::Sequential => value,
            NetworkIdMode::Hashed => *self.hashes.get(&value)?,
        };
        self.get(sequential)
    }

    /// Resolves either runtime-id mode only when the world carrier uses this table's palette.
    pub fn for_runtime(
        &self,
        value: u32,
        mode: NetworkIdMode,
        assets: &RuntimeAssets,
    ) -> Option<BlockEntityBacking<'_>> {
        if assets.provenance().block_registry_sha256 != self.registry_sha256
            || !assets.resolve(mode, value).is_known()
        {
            return None;
        }
        let sequential = match mode {
            NetworkIdMode::Sequential => value,
            NetworkIdMode::Hashed => assets.sequential_id_for_hash(value)?,
        };
        let backing = self.get(sequential)?;
        (assets.sequential_id_for_hash(backing.network_hash) == Some(sequential)
            && (mode == NetworkIdMode::Sequential || backing.network_hash == value))
            .then_some(backing)
    }

    /// Iterates the generated identity contracts for completeness checks.
    pub fn entries(&self) -> &[BlockEntityEntry] {
        &self.payload.entries
    }
}
