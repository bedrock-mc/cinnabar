use super::*;

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
