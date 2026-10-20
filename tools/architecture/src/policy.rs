use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Policy {
    pub(super) production_rust_max: usize,
    pub(super) module_root_max: usize,
    pub(super) powershell_max: usize,
    pub(super) test_max: usize,
    /// Ownership table that must describe every crate registered by this policy.
    #[serde(default)]
    pub(super) workspace_document: Option<String>,
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
    #[serde(default)]
    pub(super) reexport_allowances: Vec<ReexportAllowance>,
    #[serde(default)]
    pub(super) timing: TimingRules,
    #[serde(default)]
    pub(super) identifier_literals: IdentifierRules,
    #[serde(default)]
    pub(super) identifier_literal_exceptions: Vec<IdentifierException>,
}

/// Identifier sources, their owners, and the remaining per-file migration budgets.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentifierRules {
    #[serde(default)]
    pub(super) owners: Vec<String>,
    #[serde(default)]
    pub(super) vocabularies: Vec<IdentifierVocabulary>,
    #[serde(default)]
    pub(super) block_registries: Vec<String>,
    #[serde(default)]
    pub(super) baselines: Vec<LineBaseline>,
}

/// A pinned text vocabulary and its zero-based identifier column.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentifierVocabulary {
    pub(super) path: String,
    pub(super) column: usize,
}

/// A literal allowed in one file for a documented reason.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentifierException {
    pub(super) path: String,
    pub(super) literal: String,
    pub(super) reason: String,
}

/// Tests never sleep, and listed crates never read the clock outside tests.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TimingRules {
    #[serde(default)]
    pub(super) clock_free_crates: Vec<String>,
    #[serde(default)]
    pub(super) sleep_exceptions: Vec<SleepException>,
}

/// A test file that may sleep until it is converted; the reason says why.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SleepException {
    pub(super) path: String,
    pub(super) reason: String,
}

/// Temporary, individually named forwarding APIs retained during an ownership migration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReexportAllowance {
    pub(super) path: String,
    pub(super) exports: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LineBaseline {
    pub(super) path: String,
    pub(super) max: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VendoredRule {
    pub(super) path: String,
    pub(super) ownership_record: String,
    /// The only crate that may depend on packages in this tree, by any dependency kind.
    #[serde(default)]
    pub(super) owner: Option<String>,
}

/// Original art exempt from `forbidden_artifacts`; its record must name the path.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnedArtifact {
    pub(super) path: String,
    pub(super) ownership_record: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// Limits all Rust source lines in a shared hub.
    #[serde(default)]
    pub(super) max_rust_lines: Option<usize>,
    /// Allows only data declarations and module wiring, with no executable items.
    #[serde(default)]
    pub(super) contract_only: bool,
    /// Reject these packages through local dependency paths, including this crate's tests.
    #[serde(default)]
    pub(super) forbidden_transitive_dependencies: Vec<String>,
    /// Fixture features must stay off defaults and production dependency declarations.
    #[serde(default)]
    pub(super) test_support_features: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub(super) struct OwnershipException {
    pub(super) path: String,
    pub(super) owner: String,
    pub(super) owned_type: String,
}

#[cfg(test)]
mod tests;
