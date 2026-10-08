use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct Policy {
    pub(super) production_rust_max: usize,
    pub(super) module_root_max: usize,
    pub(super) powershell_max: usize,
    pub(super) test_max: usize,
    #[serde(default)]
    pub(super) vendored: Vec<VendoredRule>,
    #[serde(default)]
    pub(super) forbidden_artifacts: Vec<String>,
    #[serde(default)]
    pub(super) owned_artifacts: Vec<OwnedArtifact>,
    #[serde(default)]
    pub(super) line_baselines: Vec<LineBaseline>,
    #[serde(default, rename = "crates")]
    pub(super) crate_rules: Vec<CrateRule>,
    #[serde(default)]
    pub(super) markers: Vec<MarkerRule>,
    #[serde(default)]
    pub(super) module_boundaries: Vec<ModuleBoundary>,
}

#[derive(Debug, Deserialize)]
pub(super) struct LineBaseline {
    pub(super) path: String,
    pub(super) max: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct VendoredRule {
    pub(super) path: String,
    pub(super) ownership_record: String,
}

/// Original art exempt from `forbidden_artifacts`; its record must name the path.
#[derive(Debug, Deserialize)]
pub(super) struct OwnedArtifact {
    pub(super) path: String,
    pub(super) ownership_record: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CrateRule {
    pub(super) name: String,
    pub(super) path: String,
    #[serde(default)]
    pub(super) allowed_dependencies: Vec<String>,
    #[serde(default)]
    pub(super) forbidden_dependencies: Vec<String>,
    /// Reject every dependency, including external and development dependencies.
    #[serde(default)]
    pub(super) dependency_free: bool,
    /// Reject these packages through local dependency paths, including this crate's tests.
    #[serde(default)]
    pub(super) forbidden_transitive_dependencies: Vec<String>,
    /// Fixture features must stay off defaults and production dependency declarations.
    #[serde(default)]
    pub(super) test_support_features: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MarkerRule {
    pub(super) literal: String,
    pub(super) kind: MarkerKind,
    pub(super) producer: String,
    pub(super) consumer: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(super) enum MarkerKind {
    Parsed,
    LogOnly,
    HarnessOnly,
    EnvironmentVariable,
}

/// Production module paths and authoritative owned types forbidden at one boundary.
#[derive(Debug, Deserialize)]
pub(super) struct ModuleBoundary {
    pub(super) path: String,
    #[serde(default)]
    pub(super) forbidden_modules: Vec<String>,
    /// Root module names forbidden through crate-relative paths, including super paths.
    #[serde(default)]
    pub(super) forbidden_crate_modules: Vec<String>,
    #[serde(default)]
    pub(super) forbidden_owned_types: Vec<String>,
    #[serde(default)]
    pub(super) ownership_exceptions: Vec<OwnershipException>,
}

/// A named immutable presentation snapshot may own a copy of one specific type.
#[derive(Debug, Deserialize)]
pub(super) struct OwnershipException {
    pub(super) path: String,
    pub(super) owner: String,
    pub(super) owned_type: String,
}
