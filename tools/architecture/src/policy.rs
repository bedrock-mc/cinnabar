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
    #[serde(default, rename = "crates")]
    pub(super) crate_rules: Vec<CrateRule>,
    #[serde(default)]
    pub(super) markers: Vec<MarkerRule>,
}

#[derive(Debug, Deserialize)]
pub(super) struct VendoredRule {
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
