use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use assets::{
    BlockFace, BlockFlags, ContributorRole, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_WATER_TINT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_PANE,
    MODEL_TEMPLATE_FLAG_STAIR, ModelFamily, ModelStateField, NetworkIdMode, RegistryRecord,
    RuntimeAssets, TextureRef, VisualKind, read_registry,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const BASELINE_SCHEMA: &str = "cinnabar-visual-coverage-baseline-v1";
pub const REPORT_SCHEMA: &str = "cinnabar-visual-coverage-report-v1";
pub const STRICT_REPORT_SCHEMA: &str = "cinnabar-visual-coverage-strict-v1";
pub const GALLERY_INVENTORY_SCHEMA: &str = "cinnabar-gallery-inventory-v1";
pub const GALLERY_PAGE_CAPACITY: usize = 256;
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderStream {
    NoDraw,
    Cube,
    Model,
    Liquid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StrictStateRoute {
    pub state: StateIdentity,
    pub visual_kind: String,
    pub render_stream: RenderStream,
    pub material_ids: Vec<u32>,
    pub model_template: Option<u32>,
    pub animation_ids: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StrictReport {
    pub schema: &'static str,
    pub protocol: u32,
    pub registry_sha256: String,
    pub assets_sha256: String,
    pub counts: Counts,
    pub routes: Vec<StrictStateRoute>,
    pub invisible_decisions: Vec<InvisibleDecision>,
    pub states_by_stream: BTreeMap<RenderStream, usize>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GalleryTargetStatus {
    Drawable,
    Invisible,
    Diagnostic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GalleryTarget {
    pub sequential_id: u32,
    pub network_hash: u32,
    pub name: String,
    pub canonical_state: String,
    pub model_family: String,
    pub is_air: bool,
    pub status: GalleryTargetStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GalleryPage {
    pub index: u32,
    pub first_sequential_id: u32,
    pub last_sequential_id: u32,
    pub targets: Vec<GalleryTarget>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GalleryInventory {
    pub schema: String,
    pub protocol: u32,
    pub registry_sha256: String,
    pub assets_sha256: String,
    pub baseline_sha256: String,
    pub accepting: bool,
    pub diagnostic_targets: usize,
    pub target_count: usize,
    pub pages: Vec<GalleryPage>,
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
    #[error("non-air diagnostic visual for state {state:?}")]
    NonAirDiagnostic { state: StateIdentity },
    #[error("unsupported model family {family} for state {state:?}")]
    UnsupportedModelFamily {
        state: StateIdentity,
        family: String,
    },
    #[error("invalid air route {kind} for state {state:?}")]
    InvalidAirRoute { state: StateIdentity, kind: String },
    #[error("invalid invisible route {kind} for state {state:?}")]
    InvalidInvisibleRoute { state: StateIdentity, kind: String },
    #[error("empty or diagnostic {kind} route for state {state:?}")]
    EmptyVisibleRoute { state: StateIdentity, kind: String },
    #[error("state {state:?} references diagnostic material {material_id}")]
    DiagnosticMaterialReference {
        state: StateIdentity,
        material_id: u32,
    },
    #[error("state {state:?} material {material_id} references diagnostic texture")]
    DiagnosticTextureReference {
        state: StateIdentity,
        material_id: u32,
    },
    #[error("state {state:?} references empty animation {animation_id}")]
    EmptyAnimation {
        state: StateIdentity,
        animation_id: u32,
    },
    #[error("state {state:?} animation {animation_id} references diagnostic frame texture")]
    DiagnosticAnimationFrameReference {
        state: StateIdentity,
        animation_id: u32,
    },
    #[error("liquid state {state:?} has invalid depth variant {variant}")]
    InvalidLiquidDepth { state: StateIdentity, variant: u32 },
    #[error("liquid state {state:?} has mixed or unsupported materials {material_ids:?}")]
    InvalidLiquidMaterials {
        state: StateIdentity,
        material_ids: Vec<u32>,
    },
    #[error("failed to atomically write report {path}: {source}")]
    ReportWrite {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
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

/// Validates the complete semantic draw graph for decoded records.
///
/// `enforce_protocol_1001` is false only for bounded synthetic fixtures. All
/// production callers must use [`strict_bytes`], which enforces the exact
/// reviewed protocol inventory and baseline.
pub fn strict_records(
    records: &[RegistryRecord],
    runtime: &RuntimeAssets,
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
    enforce_protocol_1001: bool,
) -> Result<StrictReport, CoverageError> {
    let ratchet_report = if enforce_protocol_1001 {
        ratchet_protocol_1001(snapshot, baseline)?
    } else {
        ratchet(snapshot, baseline)?
    };

    let mut ordered = records.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|record| record.sequential_id);
    let mut routes = Vec::with_capacity(ordered.len());
    let mut states_by_stream = BTreeMap::from([
        (RenderStream::NoDraw, 0),
        (RenderStream::Cube, 0),
        (RenderStream::Model, 0),
        (RenderStream::Liquid, 0),
    ]);

    for record in ordered {
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

        let state = StateIdentity::from_record(record);
        let kind = visual_kind_name(sequential.kind()).to_owned();
        if record.model_family == ModelFamily::Unknown {
            return Err(CoverageError::UnsupportedModelFamily {
                state,
                family: model_family_name(record.model_family).to_owned(),
            });
        }

        if record.flags.contains(BlockFlags::AIR) {
            if sequential.kind() != VisualKind::Invisible
                || sequential.contributor_role() != ContributorRole::Air
                || !sequential.flags().contains(BlockFlags::AIR)
                || sequential.model_template().is_some()
                || sequential.animation().is_some()
                || BlockFace::ALL
                    .into_iter()
                    .any(|face| sequential.face(face).material_id() != DIAGNOSTIC_MATERIAL)
            {
                return Err(CoverageError::InvalidAirRoute { state, kind });
            }
            push_strict_route(
                &mut routes,
                &mut states_by_stream,
                StrictStateRoute {
                    state,
                    visual_kind: kind,
                    render_stream: RenderStream::NoDraw,
                    material_ids: Vec::new(),
                    model_template: None,
                    animation_ids: Vec::new(),
                },
            );
            continue;
        }

        if sequential.kind() == VisualKind::Diagnostic {
            return Err(CoverageError::NonAirDiagnostic { state });
        }

        let mut material_ids = BTreeSet::new();
        let mut animation_ids = BTreeSet::new();
        let (render_stream, model_template) = match sequential.kind() {
            VisualKind::Invisible => {
                if sequential.contributor_role() == ContributorRole::Air
                    || sequential.flags().contains(BlockFlags::AIR)
                    || sequential.model_template().is_some()
                    || sequential.animation().is_some()
                    || BlockFace::ALL
                        .into_iter()
                        .any(|face| sequential.face(face).material_id() != DIAGNOSTIC_MATERIAL)
                {
                    return Err(CoverageError::InvalidInvisibleRoute { state, kind });
                }
                (RenderStream::NoDraw, None)
            }
            VisualKind::Cube => {
                for face in BlockFace::ALL {
                    let material_id = sequential.face(face).material_id();
                    if material_id == DIAGNOSTIC_MATERIAL {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    }
                    material_ids.insert(material_id);
                }
                (RenderStream::Cube, None)
            }
            VisualKind::Cross | VisualKind::Model => {
                let Some(template_id) = sequential.model_template() else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let template_start = template_id as usize;
                let Some(base_template) = runtime.model_templates().get(template_start) else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let (template_count, allowed_empty_offset) =
                    if base_template.flags & MODEL_TEMPLATE_FLAG_STAIR != 0 {
                        (5, None)
                    } else if base_template.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                        (2, None)
                    } else if base_template.flags & MODEL_TEMPLATE_FLAG_PANE != 0 {
                        (16, None)
                    } else if base_template.flags
                        & (MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER)
                        != 0
                    {
                        (17, Some(1))
                    } else {
                        (1, None)
                    };
                let Some(template_end) = template_start.checked_add(template_count) else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                let Some(templates) = runtime.model_templates().get(template_start..template_end)
                else {
                    return Err(CoverageError::EmptyVisibleRoute { state, kind });
                };
                for (offset, template) in templates.iter().enumerate() {
                    if template.quad_count == 0 && allowed_empty_offset != Some(offset) {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    }
                    let start = template.quad_start as usize;
                    let Some(end) = start.checked_add(template.quad_count as usize) else {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    };
                    let Some(quads) = runtime.model_quads().get(start..end) else {
                        return Err(CoverageError::EmptyVisibleRoute { state, kind });
                    };
                    for quad in quads {
                        if quad.material == DIAGNOSTIC_MATERIAL {
                            return Err(CoverageError::DiagnosticMaterialReference {
                                state,
                                material_id: quad.material,
                            });
                        }
                        material_ids.insert(quad.material);
                    }
                }
                (RenderStream::Model, Some(template_id))
            }
            VisualKind::Liquid => {
                if sequential.variant() > 15 {
                    return Err(CoverageError::InvalidLiquidDepth {
                        state,
                        variant: sequential.variant(),
                    });
                }
                for face in BlockFace::ALL {
                    let material_id = sequential.face(face).material_id();
                    if material_id == DIAGNOSTIC_MATERIAL {
                        return Err(CoverageError::DiagnosticMaterialReference {
                            state,
                            material_id,
                        });
                    }
                    material_ids.insert(material_id);
                }
                let all_water = material_ids
                    .iter()
                    .all(|&id| material_is_water(runtime, id));
                let all_lava = material_ids
                    .iter()
                    .all(|&id| material_is_depth_writing_liquid(runtime, id));
                if !all_water && !all_lava {
                    return Err(CoverageError::InvalidLiquidMaterials {
                        state,
                        material_ids: material_ids.iter().copied().collect(),
                    });
                }
                (RenderStream::Liquid, None)
            }
            VisualKind::Diagnostic => unreachable!("diagnostic handled above"),
        };

        if let Some(animation_id) = sequential.animation() {
            animation_ids.insert(animation_id);
        }
        for &material_id in &material_ids {
            validate_reached_material(runtime, &state, material_id, &mut animation_ids)?;
        }
        for &animation_id in &animation_ids {
            validate_reached_animation(runtime, &state, animation_id)?;
        }

        push_strict_route(
            &mut routes,
            &mut states_by_stream,
            StrictStateRoute {
                state,
                visual_kind: kind,
                render_stream,
                material_ids: material_ids.into_iter().collect(),
                model_template,
                animation_ids: animation_ids.into_iter().collect(),
            },
        );
    }

    Ok(StrictReport {
        schema: STRICT_REPORT_SCHEMA,
        protocol: ratchet_report.protocol,
        registry_sha256: ratchet_report.registry_sha256,
        assets_sha256: ratchet_report.assets_sha256,
        counts: ratchet_report.counts,
        routes,
        invisible_decisions: ratchet_report.invisible_decisions,
        states_by_stream,
    })
}

pub fn strict_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
    baseline: &Baseline,
) -> Result<StrictReport, CoverageError> {
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    let snapshot = analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )?;
    strict_records(&records, &runtime, snapshot, baseline, true)
}

/// Compiles the exact protocol-1001 target inventory consumed by later gallery
/// placement and screenshot stages. Diagnostics are retained as explicit
/// targets. Acceptance requires both zero diagnostics and a fully valid strict
/// semantic draw graph.
fn assemble_gallery_inventory(
    snapshot: CoverageSnapshot,
    baseline: &Baseline,
    baseline_sha256: &str,
    strict_semantics_valid: bool,
) -> Result<GalleryInventory, CoverageError> {
    let report = ratchet_protocol_1001(snapshot, baseline)?;
    let diagnostic_ids = report
        .diagnostic_states
        .iter()
        .map(|state| state.sequential_id)
        .collect::<BTreeSet<_>>();
    let invisible_ids = report
        .invisible_decisions
        .iter()
        .map(|decision| decision.state.sequential_id)
        .collect::<BTreeSet<_>>();
    let targets = report
        .states
        .into_iter()
        .map(|state| {
            let status = if diagnostic_ids.contains(&state.sequential_id) {
                GalleryTargetStatus::Diagnostic
            } else if invisible_ids.contains(&state.sequential_id) {
                GalleryTargetStatus::Invisible
            } else {
                GalleryTargetStatus::Drawable
            };
            GalleryTarget {
                sequential_id: state.sequential_id,
                network_hash: state.network_hash,
                name: state.name,
                canonical_state: state.canonical_state,
                model_family: state.model_family,
                is_air: state.is_air,
                status,
            }
        })
        .collect::<Vec<_>>();
    let pages = targets
        .chunks(GALLERY_PAGE_CAPACITY)
        .enumerate()
        .map(|(index, targets)| GalleryPage {
            index: index as u32,
            first_sequential_id: targets
                .first()
                .expect("canonical protocol inventory is non-empty")
                .sequential_id,
            last_sequential_id: targets
                .last()
                .expect("canonical protocol inventory is non-empty")
                .sequential_id,
            targets: targets.to_vec(),
        })
        .collect();
    let diagnostic_targets = diagnostic_ids.len();
    Ok(GalleryInventory {
        schema: GALLERY_INVENTORY_SCHEMA.to_owned(),
        protocol: report.protocol,
        registry_sha256: report.registry_sha256,
        assets_sha256: report.assets_sha256,
        baseline_sha256: baseline_sha256.to_owned(),
        accepting: diagnostic_targets == 0 && strict_semantics_valid,
        diagnostic_targets,
        target_count: PROTOCOL_1001_COUNTS.states,
        pages,
    })
}

pub fn gallery_inventory_bytes(
    registry_bytes: &[u8],
    assets_bytes: &[u8],
    baseline_bytes: &[u8],
) -> Result<GalleryInventory, CoverageError> {
    let baseline = parse_baseline(baseline_bytes)?;
    let records = read_registry(registry_bytes).map_err(CoverageError::Registry)?;
    let runtime = RuntimeAssets::decode(assets_bytes).map_err(CoverageError::Assets)?;
    let snapshot = analyze_records(
        &records,
        &runtime,
        &sha256(registry_bytes),
        &sha256(assets_bytes),
    )?;
    let strict_semantics_valid =
        strict_records(&records, &runtime, snapshot.clone(), &baseline, true).is_ok();
    assemble_gallery_inventory(
        snapshot,
        &baseline,
        &sha256(baseline_bytes),
        strict_semantics_valid,
    )
}

fn push_strict_route(
    routes: &mut Vec<StrictStateRoute>,
    states_by_stream: &mut BTreeMap<RenderStream, usize>,
    route: StrictStateRoute,
) {
    *states_by_stream.entry(route.render_stream).or_default() += 1;
    routes.push(route);
}

fn validate_reached_material(
    runtime: &RuntimeAssets,
    state: &StateIdentity,
    material_id: u32,
    animation_ids: &mut BTreeSet<u32>,
) -> Result<(), CoverageError> {
    if material_id == DIAGNOSTIC_MATERIAL {
        return Err(CoverageError::DiagnosticMaterialReference {
            state: state.clone(),
            material_id,
        });
    }
    let material = runtime.material(material_id);
    if material.texture == TextureRef::DIAGNOSTIC {
        return Err(CoverageError::DiagnosticTextureReference {
            state: state.clone(),
            material_id,
        });
    }
    if material.animation != assets::NO_ANIMATION {
        animation_ids.insert(material.animation);
    }
    Ok(())
}

fn validate_reached_animation(
    runtime: &RuntimeAssets,
    state: &StateIdentity,
    animation_id: u32,
) -> Result<(), CoverageError> {
    let Some(animation) = runtime.animations().get(animation_id as usize) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    if animation.frame_count == 0 {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    }
    let start = animation.frame_start as usize;
    let Some(end) = start.checked_add(animation.frame_count as usize) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    let Some(frames) = runtime.animation_frames().get(start..end) else {
        return Err(CoverageError::EmptyAnimation {
            state: state.clone(),
            animation_id,
        });
    };
    if frames.contains(&TextureRef::DIAGNOSTIC) {
        return Err(CoverageError::DiagnosticAnimationFrameReference {
            state: state.clone(),
            animation_id,
        });
    }
    Ok(())
}

fn material_is_water(runtime: &RuntimeAssets, material_id: u32) -> bool {
    let required = MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_WATER_TINT;
    runtime.material(material_id).flags & required == required
}

fn material_is_depth_writing_liquid(runtime: &RuntimeAssets, material_id: u32) -> bool {
    runtime.material(material_id).flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE != 0
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

static ATOMIC_REPORT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Serializes deterministic JSON completely before atomically replacing the
/// destination through a unique same-directory temporary file.
pub fn write_deterministic_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<(), CoverageError> {
    let bytes = deterministic_json(value)?;
    write_report_atomic(path, &bytes).map_err(|source| CoverageError::ReportWrite {
        path: path.to_path_buf(),
        source,
    })
}

fn write_report_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "report path has no file name")
    })?;
    let mut temporary = None;
    for _ in 0..128 {
        let sequence = ATOMIC_REPORT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{}.tmp-{}-{sequence}",
            file_name.to_string_lossy(),
            std::process::id(),
        ));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(source),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve an atomic report temporary file",
        )
    })?;
    let write_result = file
        .write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(source) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(source);
    }
    if let Err(source) = replace_report_atomic(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(source);
    }
    Ok(())
}

#[cfg(windows)]
fn replace_report_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 1;
    const MOVEFILE_WRITE_THROUGH: u32 = 8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let temporary = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: Both pointers reference live, NUL-terminated UTF-16 buffers for
    // the duration of the call, and the flags request a same-volume atomic
    // replacement with write-through durability.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_report_atomic(temporary: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(temporary, destination)
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

fn visual_kind_name(kind: VisualKind) -> &'static str {
    match kind {
        VisualKind::Diagnostic => "diagnostic",
        VisualKind::Cube => "cube",
        VisualKind::Cross => "cross",
        VisualKind::Model => "model",
        VisualKind::Liquid => "liquid",
        VisualKind::Invisible => "invisible",
    }
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
        ModelFamily::GlowLichen => "glow_lichen",
        ModelFamily::SculkVein => "sculk_vein",
        ModelFamily::ChiseledBookshelf => "chiseled_bookshelf",
        ModelFamily::ResinClump => "resin_clump",
    }
}
