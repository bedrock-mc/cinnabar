use std::collections::{BTreeMap, BTreeSet};

use assets::{
    BlockFlags, ModelFamily, ModelStateField, NetworkIdMode, RegistryRecord, RuntimeAssets,
    VisualKind, read_registry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const BASELINE_SCHEMA: &str = "cinnabar-visual-coverage-baseline-v1";
pub const REPORT_SCHEMA: &str = "cinnabar-visual-coverage-report-v1";
pub const PROTOCOL: u32 = 1001;
pub const PROTOCOL_1001_COUNTS: Counts = Counts {
    names: 1_356,
    states: 16_913,
    air: 1,
};
pub const MAX_BASELINE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub names: usize,
    pub states: usize,
    pub air: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateIdentity {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub name: String,
    pub canonical_state: String,
    pub model_family: String,
    pub is_air: bool,
}

impl StateIdentity {
    #[must_use]
    pub fn from_record(record: &RegistryRecord) -> Self {
        Self {
            sequential_id: record.sequential_id,
            network_hash: record.network_hash,
            name: record.name.to_string(),
            canonical_state: record.canonical_state.to_string(),
            model_family: model_family_name(record.model_family).to_owned(),
            is_air: record.flags.contains(BlockFlags::AIR),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AllowlistEntry {
    pub state: StateIdentity,
    pub authority: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub schema: String,
    pub protocol: u32,
    pub registry_sha256: String,
    pub counts: Counts,
    pub states: Vec<StateIdentity>,
    /// Exact sequential IDs that were diagnostic when this reviewed baseline
    /// was generated. Identities live once in `states`, keeping the checked
    /// protocol inventory compact while preserving exact diffs.
    pub diagnostic_sequential_ids: Vec<u32>,
    pub invisible_allowlist: Vec<AllowlistEntry>,
    pub expected_vine_diagnostic_masks: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InvisibleDecision {
    pub state: StateIdentity,
    pub allowed: bool,
    pub authority: String,
    pub source: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CoverageSnapshot {
    pub protocol: u32,
    pub registry_sha256: String,
    pub assets_sha256: String,
    pub counts: Counts,
    pub states: Vec<StateIdentity>,
    pub diagnostic_states: Vec<StateIdentity>,
    pub diagnostics_by_family: BTreeMap<String, usize>,
    pub diagnostics_by_name: BTreeMap<String, usize>,
    pub invisible_states: Vec<StateIdentity>,
    pub air_states: Vec<StateIdentity>,
    pub vine_diagnostic_masks: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RatchetReport {
    pub schema: &'static str,
    pub protocol: u32,
    pub registry_sha256: String,
    pub assets_sha256: String,
    pub counts: Counts,
    pub states: Vec<StateIdentity>,
    pub diagnostic_states: Vec<StateIdentity>,
    pub diagnostics_by_family: BTreeMap<String, usize>,
    pub diagnostics_by_name: BTreeMap<String, usize>,
    pub added_diagnostics: Vec<StateIdentity>,
    pub removed_diagnostics: Vec<StateIdentity>,
    pub invisible_decisions: Vec<InvisibleDecision>,
    pub vine_diagnostic_masks: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum CoverageError {
    #[error("registry decode failed: {0}")]
    Registry(#[source] assets::AssetError),
    #[error("asset blob decode failed: {0}")]
    Assets(#[source] assets::AssetError),
    #[error("duplicate sequential ID {0}")]
    DuplicateSequentialId(u32),
    #[error("sequential IDs are not contiguous: expected {expected}, found {actual}")]
    NonContiguousSequentialId { expected: u32, actual: u32 },
    #[error("duplicate canonical state identity: {name}|{canonical_state}")]
    DuplicateCanonicalState {
        name: String,
        canonical_state: String,
    },
    #[error("sequential/hashed lookup mismatch for ID {sequential_id} hash {network_hash:#010x}")]
    LookupMismatch {
        sequential_id: u32,
        network_hash: u32,
    },
    #[error(
        "runtime asset cardinality differs from BREG: registry={registry}, visuals={visuals}, hashes={hashes}"
    )]
    RuntimeCardinalityMismatch {
        registry: usize,
        visuals: usize,
        hashes: usize,
    },
    #[error("baseline schema/protocol is unsupported")]
    UnsupportedBaseline,
    #[error("protocol-1001 inventory is not canonical: {0}")]
    NonCanonicalProtocolInventory(&'static str),
    #[error("baseline exceeds the {MAX_BASELINE_BYTES}-byte input ceiling")]
    BaselineTooLarge,
    #[error("baseline is not in canonical sorted order or contains duplicates")]
    NonCanonicalBaseline,
    #[error(
        "cannot generate a baseline while a non-air invisible state lacks a source citation: {state:?}"
    )]
    MissingInvisibleCitation { state: StateIdentity },
    #[error("registry hash differs: expected {expected}, found {actual}")]
    RegistryHashMismatch { expected: String, actual: String },
    #[error("registry inventory counts differ: expected {expected:?}, found {actual:?}")]
    CountMismatch { expected: Counts, actual: Counts },
    #[error("canonical state inventory differs from the reviewed baseline")]
    StateInventoryMismatch,
    #[error("new diagnostic states: {states:?}")]
    DiagnosticRegression { states: Vec<StateIdentity> },
    #[error("invisible state is not source-cited in the reviewed allowlist: {state:?}")]
    UnreviewedInvisible { state: StateIdentity },
    #[error("invisible allowlist citation is missing or invalid: {state:?}")]
    InvalidInvisibleCitation { state: StateIdentity },
    #[error("invisible allowlist entry no longer resolves to an invisible state: {state:?}")]
    StaleInvisibleAllowlist { state: StateIdentity },
    #[error("vine diagnostic masks differ: expected {expected:?}, found {actual:?}")]
    VineDiagnosticsMismatch { expected: Vec<u8>, actual: Vec<u8> },
    #[error("JSON encode/decode failed: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn baseline_from_snapshot(
    snapshot: &CoverageSnapshot,
    mut invisible_allowlist: Vec<AllowlistEntry>,
) -> Result<Baseline, CoverageError> {
    validate_protocol_snapshot(snapshot)?;
    invisible_allowlist.sort_by(|left, right| left.state.cmp(&right.state));
    let allowlisted = invisible_allowlist
        .iter()
        .map(|entry| entry.state.clone())
        .collect::<BTreeSet<_>>();
    let air = snapshot.air_states.iter().cloned().collect::<BTreeSet<_>>();
    let invisible = snapshot
        .invisible_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for entry in &invisible_allowlist {
        if entry.authority.trim().is_empty()
            || !entry.source.starts_with("https://")
            || entry.source.len() <= "https://".len()
        {
            return Err(CoverageError::InvalidInvisibleCitation {
                state: entry.state.clone(),
            });
        }
        if !invisible.contains(&entry.state) {
            return Err(CoverageError::StaleInvisibleAllowlist {
                state: entry.state.clone(),
            });
        }
    }
    for state in &snapshot.invisible_states {
        if !air.contains(state) && !allowlisted.contains(state) {
            return Err(CoverageError::MissingInvisibleCitation {
                state: state.clone(),
            });
        }
    }
    let baseline = Baseline {
        schema: BASELINE_SCHEMA.to_owned(),
        protocol: snapshot.protocol,
        registry_sha256: snapshot.registry_sha256.clone(),
        counts: snapshot.counts,
        states: snapshot.states.clone(),
        diagnostic_sequential_ids: snapshot
            .diagnostic_states
            .iter()
            .map(|state| state.sequential_id)
            .collect(),
        invisible_allowlist,
        expected_vine_diagnostic_masks: snapshot.vine_diagnostic_masks.clone(),
    };
    validate_baseline(&baseline)?;
    Ok(baseline)
}

pub fn analyze_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
) -> Result<CoverageSnapshot, CoverageError> {
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )
}

pub fn analyze_records(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
    registry_sha256: &str,
    assets_sha256: &str,
) -> Result<CoverageSnapshot, CoverageError> {
    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|record| record.sequential_id);

    let mut previous = None;
    let mut canonical = BTreeSet::new();
    for (expected, record) in ordered.iter().enumerate() {
        if previous == Some(record.sequential_id) {
            return Err(CoverageError::DuplicateSequentialId(record.sequential_id));
        }
        previous = Some(record.sequential_id);
        let expected = u32::try_from(expected).expect("registry is bounded below u32::MAX");
        if record.sequential_id != expected {
            return Err(CoverageError::NonContiguousSequentialId {
                expected,
                actual: record.sequential_id,
            });
        }
        let key = (record.name.to_string(), record.canonical_state.to_string());
        if !canonical.insert(key.clone()) {
            return Err(CoverageError::DuplicateCanonicalState {
                name: key.0,
                canonical_state: key.1,
            });
        }
    }

    if runtime.visual_count() != records.len() || runtime.hashed_count() != records.len() {
        return Err(CoverageError::RuntimeCardinalityMismatch {
            registry: records.len(),
            visuals: runtime.visual_count(),
            hashes: runtime.hashed_count(),
        });
    }

    let mut diagnostics = Vec::new();
    let mut invisible = Vec::new();
    let mut air_states = Vec::new();
    let mut diagnostics_by_family = BTreeMap::new();
    let mut diagnostics_by_name = BTreeMap::new();
    let mut vine_masks = Vec::new();
    for record in &ordered {
        if runtime.sequential_id_for_hash(record.network_hash) != Some(record.sequential_id) {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }
        let sequential = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        let hashed = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        if !sequential.is_known() || !hashed.is_known() || sequential != hashed {
            return Err(CoverageError::LookupMismatch {
                sequential_id: record.sequential_id,
                network_hash: record.network_hash,
            });
        }
        let identity = StateIdentity::from_record(record);
        if identity.is_air {
            air_states.push(identity.clone());
        }
        match sequential.kind() {
            VisualKind::Diagnostic => {
                *diagnostics_by_family
                    .entry(identity.model_family.clone())
                    .or_insert(0) += 1;
                *diagnostics_by_name
                    .entry(identity.name.clone())
                    .or_insert(0) += 1;
                if record.name.as_ref() == "minecraft:vine"
                    && let Some(mask) = record.model_state.get(ModelStateField::Connections)
                    && let Ok(mask) = u8::try_from(mask)
                {
                    vine_masks.push(mask);
                }
                diagnostics.push(identity);
            }
            VisualKind::Invisible => invisible.push(identity),
            _ => {}
        }
    }
    diagnostics.sort();
    invisible.sort();
    air_states.sort();
    vine_masks.sort_unstable();
    vine_masks.dedup();

    let names = records
        .iter()
        .map(|record| record.name.as_ref())
        .collect::<BTreeSet<_>>()
        .len();
    let air = records
        .iter()
        .filter(|record| record.flags.contains(BlockFlags::AIR))
        .count();
    Ok(CoverageSnapshot {
        protocol: PROTOCOL,
        registry_sha256: registry_sha256.to_owned(),
        assets_sha256: assets_sha256.to_owned(),
        counts: Counts {
            names,
            states: records.len(),
            air,
        },
        states: ordered
            .into_iter()
            .map(StateIdentity::from_record)
            .collect(),
        diagnostic_states: diagnostics,
        diagnostics_by_family,
        diagnostics_by_name,
        invisible_states: invisible,
        air_states,
        vine_diagnostic_masks: vine_masks,
    })
}

pub fn ratchet(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
) -> Result<RatchetReport, CoverageError> {
    validate_baseline(baseline)?;
    if snapshot.registry_sha256 != baseline.registry_sha256 {
        return Err(CoverageError::RegistryHashMismatch {
            expected: baseline.registry_sha256.clone(),
            actual: snapshot.registry_sha256,
        });
    }
    if snapshot.counts != baseline.counts {
        return Err(CoverageError::CountMismatch {
            expected: baseline.counts,
            actual: snapshot.counts,
        });
    }
    if snapshot.states != baseline.states {
        return Err(CoverageError::StateInventoryMismatch);
    }

    let old = baseline
        .diagnostic_sequential_ids
        .iter()
        .map(|&sequential_id| baseline.states[sequential_id as usize].clone())
        .collect::<BTreeSet<_>>();
    let current = snapshot
        .diagnostic_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let added = current.difference(&old).cloned().collect::<Vec<_>>();
    if !added.is_empty() {
        return Err(CoverageError::DiagnosticRegression { states: added });
    }
    let removed = old.difference(&current).cloned().collect::<Vec<_>>();

    if snapshot.vine_diagnostic_masks != baseline.expected_vine_diagnostic_masks {
        return Err(CoverageError::VineDiagnosticsMismatch {
            expected: baseline.expected_vine_diagnostic_masks.clone(),
            actual: snapshot.vine_diagnostic_masks,
        });
    }

    let invisible_set = snapshot
        .invisible_states
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut allowlist = BTreeMap::new();
    for entry in &baseline.invisible_allowlist {
        if entry.authority.trim().is_empty()
            || !entry.source.starts_with("https://")
            || entry.source.len() <= "https://".len()
        {
            return Err(CoverageError::InvalidInvisibleCitation {
                state: entry.state.clone(),
            });
        }
        if allowlist.insert(entry.state.clone(), entry).is_some() {
            return Err(CoverageError::NonCanonicalBaseline);
        }
        if !invisible_set.contains(&entry.state) {
            return Err(CoverageError::StaleInvisibleAllowlist {
                state: entry.state.clone(),
            });
        }
    }

    let air_states = snapshot.air_states.iter().cloned().collect::<BTreeSet<_>>();
    let mut invisible_decisions = Vec::with_capacity(snapshot.invisible_states.len());
    for state in &snapshot.invisible_states {
        if air_states.contains(state) {
            invisible_decisions.push(InvisibleDecision {
                state: state.clone(),
                allowed: true,
                authority: "BREG1003 AIR flag".into(),
                source: "registry://BREG1003/air".into(),
            });
        } else if let Some(entry) = allowlist.get(state) {
            invisible_decisions.push(InvisibleDecision {
                state: state.clone(),
                allowed: true,
                authority: entry.authority.clone(),
                source: entry.source.clone(),
            });
        } else {
            return Err(CoverageError::UnreviewedInvisible {
                state: state.clone(),
            });
        }
    }

    Ok(RatchetReport {
        schema: REPORT_SCHEMA,
        protocol: snapshot.protocol,
        registry_sha256: snapshot.registry_sha256,
        assets_sha256: snapshot.assets_sha256,
        counts: snapshot.counts,
        states: snapshot.states,
        diagnostic_states: snapshot.diagnostic_states,
        diagnostics_by_family: snapshot.diagnostics_by_family,
        diagnostics_by_name: snapshot.diagnostics_by_name,
        added_diagnostics: Vec::new(),
        removed_diagnostics: removed,
        invisible_decisions,
        vine_diagnostic_masks: snapshot.vine_diagnostic_masks,
    })
}

/// Runs the production protocol-1001 gate. Synthetic tests may use `ratchet`
/// directly, but the CLI must never accept a caller-replaced smaller corpus.
pub fn ratchet_protocol_1001(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
) -> Result<RatchetReport, CoverageError> {
    validate_protocol_snapshot(&snapshot)?;
    validate_protocol_baseline(baseline)?;
    ratchet(snapshot, baseline)
}

pub fn parse_baseline(bytes: &[u8]) -> Result<Baseline, CoverageError> {
    if bytes.len() > MAX_BASELINE_BYTES {
        return Err(CoverageError::BaselineTooLarge);
    }
    Ok(serde_json::from_slice(bytes)?)
}

pub fn deterministic_json<T: Serialize>(value: &T) -> Result<Vec<u8>, CoverageError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_baseline(baseline: &Baseline) -> Result<(), CoverageError> {
    if baseline.schema != BASELINE_SCHEMA || baseline.protocol != PROTOCOL {
        return Err(CoverageError::UnsupportedBaseline);
    }
    if !is_strictly_sorted_u32(&baseline.diagnostic_sequential_ids)
        || !is_strictly_sorted_by_id(&baseline.states)
        || !is_strictly_sorted_u8(&baseline.expected_vine_diagnostic_masks)
        || !baseline
            .diagnostic_sequential_ids
            .iter()
            .all(|&sequential_id| (sequential_id as usize) < baseline.states.len())
        || !baseline
            .invisible_allowlist
            .windows(2)
            .all(|pair| pair[0].state < pair[1].state)
    {
        return Err(CoverageError::NonCanonicalBaseline);
    }
    Ok(())
}

fn validate_protocol_snapshot(snapshot: &CoverageSnapshot) -> Result<(), CoverageError> {
    if snapshot.protocol != PROTOCOL || snapshot.counts != PROTOCOL_1001_COUNTS {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "snapshot counts",
        ));
    }
    validate_protocol_states(&snapshot.states)
}

fn validate_protocol_baseline(baseline: &Baseline) -> Result<(), CoverageError> {
    if baseline.protocol != PROTOCOL || baseline.counts != PROTOCOL_1001_COUNTS {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "baseline counts",
        ));
    }
    validate_protocol_states(&baseline.states)
}

fn validate_protocol_states(states: &[StateIdentity]) -> Result<(), CoverageError> {
    if states.len() != PROTOCOL_1001_COUNTS.states {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "state vector length",
        ));
    }
    let names = states
        .iter()
        .map(|state| state.name.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    if names != PROTOCOL_1001_COUNTS.names {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "state name cardinality",
        ));
    }
    if states.iter().filter(|state| state.is_air).count() != PROTOCOL_1001_COUNTS.air {
        return Err(CoverageError::NonCanonicalProtocolInventory(
            "air cardinality",
        ));
    }
    Ok(())
}

fn is_strictly_sorted_by_id(values: &[StateIdentity]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, state)| state.sequential_id == index as u32)
}

fn is_strictly_sorted_u8(values: &[u8]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn is_strictly_sorted_u32(values: &[u32]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn model_family_name(family: ModelFamily) -> &'static str {
    match family {
        ModelFamily::Unknown => "unknown",
        ModelFamily::Air => "air",
        ModelFamily::Cube => "cube",
        ModelFamily::Leaves => "leaves",
        ModelFamily::Cross => "cross",
        ModelFamily::Crop => "crop",
        ModelFamily::Liquid => "liquid",
        ModelFamily::Slab => "slab",
        ModelFamily::Stair => "stair",
        ModelFamily::Door => "door",
        ModelFamily::Trapdoor => "trapdoor",
        ModelFamily::Pane => "pane",
        ModelFamily::Fence => "fence",
        ModelFamily::Gate => "gate",
        ModelFamily::Chest => "chest",
        ModelFamily::Sign => "sign",
        ModelFamily::Wall => "wall",
        ModelFamily::Bed => "bed",
        ModelFamily::Rail => "rail",
        ModelFamily::Torch => "torch",
        ModelFamily::Button => "button",
        ModelFamily::PressurePlate => "pressure_plate",
        ModelFamily::Carpet => "carpet",
        ModelFamily::Layer => "layer",
        ModelFamily::Decorative => "decorative",
        ModelFamily::Statue => "statue",
        ModelFamily::Cuboid => "cuboid",
        ModelFamily::Aquatic => "aquatic",
        ModelFamily::Cocoa => "cocoa",
        ModelFamily::Lever => "lever",
        ModelFamily::Invisible => "invisible",
        ModelFamily::FlowerBed => "flower_bed",
        ModelFamily::Vine => "vine",
    }
}
