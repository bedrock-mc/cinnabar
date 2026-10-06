use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub(crate) name: String,
    pub(crate) cargo_id: String,
    pub(crate) root: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) doctest: bool,
}

impl Package {
    #[must_use]
    pub fn new(name: &str, root: &str, dependencies: &[&str]) -> Self {
        Self {
            name: name.into(),
            cargo_id: name.into(),
            root: normalize(root),
            dependencies: dependencies.iter().map(|name| (*name).into()).collect(),
            doctest: false,
        }
    }

    pub(crate) fn from_owned(
        cargo_id: String,
        name: String,
        root: String,
        dependencies: Vec<String>,
        doctest: bool,
    ) -> Self {
        Self {
            name,
            cargo_id,
            root: normalize(&root),
            dependencies,
            doctest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Workspace,
    Packages(Vec<String>),
    NoPackages,
}

/// A Go module; modules outside `go.work` are tested with `GOWORK=off`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GoModule {
    pub(crate) dir: String,
    pub(crate) in_workspace: bool,
}

impl GoModule {
    #[must_use]
    pub fn new(dir: &str, in_workspace: bool) -> Self {
        Self {
            dir: normalize(dir),
            in_workspace,
        }
    }
}

/// Checks outside the Rust gate that a change set needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtraChecks {
    pub go_modules: Vec<GoModule>,
    pub packaging: bool,
}

/// Paths outside every crate root that a crate's tests read.
const EXTERNAL_TEST_INPUTS: &[(&str, &str)] = &[
    ("assets/java-hud", "json-ui"),
    ("plan.md", "architecture"),
    ("docs/evidence", "architecture"),
];

/// Selects Rust packages; paths no rule recognises fail safe to the whole workspace.
#[must_use]
pub fn select_packages(changed_paths: &[&str], packages: &[Package]) -> Selection {
    if changed_paths.iter().any(|path| is_workspace_input(path)) {
        return Selection::Workspace;
    }

    let mut selected = BTreeSet::new();
    for path in changed_paths {
        let path = normalize(path);
        if let Some(owner) = packages
            .iter()
            .filter(|package| is_within(&path, &package.root))
            .max_by_key(|package| package.root.len())
        {
            selected.insert(owner.name.clone());
        } else if let Some((_, reader)) = EXTERNAL_TEST_INPUTS
            .iter()
            .find(|(input, _)| is_within(&path, input))
        {
            selected.insert((*reader).to_owned());
        } else if !is_rust_free(&path) {
            return Selection::Workspace;
        }
    }

    if selected.is_empty() {
        return Selection::NoPackages;
    }

    let mut pending = VecDeque::from_iter(selected.iter().cloned());
    while let Some(changed) = pending.pop_front() {
        for package in packages {
            if package.dependencies.contains(&changed) && selected.insert(package.name.clone()) {
                pending.push_back(package.name.clone());
            }
        }
    }

    Selection::Packages(selected.into_iter().collect())
}

/// Returns the Go modules owning changed paths and whether the packaging tests apply.
/// `go.work` and any workspace module's `go.mod` or `go.sum` select every workspace module,
/// since their requirements and replaces resolve together.
#[must_use]
pub fn select_extra_checks(changed_paths: &[&str], go_modules: &[GoModule]) -> ExtraChecks {
    let mut selected = BTreeSet::new();
    let mut packaging = false;
    for path in changed_paths {
        let path = normalize(path);
        packaging |= is_within(&path, "packaging");
        let owner = go_modules
            .iter()
            .filter(|module| module.dir.is_empty() || is_within(&path, &module.dir))
            .max_by_key(|module| module.dir.len());
        let workspace_manifest = owner.is_some_and(|module| {
            module.in_workspace
                && ["go.mod", "go.sum"].iter().any(|name| {
                    path == if module.dir.is_empty() {
                        (*name).to_owned()
                    } else {
                        format!("{}/{name}", module.dir)
                    }
                })
        });
        if workspace_manifest || matches!(path.as_str(), "go.work" | "go.work.sum") {
            selected.extend(go_modules.iter().filter(|module| module.in_workspace));
        } else if let Some(owner) = owner {
            selected.insert(owner);
        }
    }
    ExtraChecks {
        go_modules: selected.into_iter().cloned().collect(),
        packaging,
    }
}

pub(crate) fn normalize(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

fn is_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_workspace_input(path: &str) -> bool {
    let path = normalize(path);
    matches!(
        path.as_str(),
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "rust-toolchain"
    ) || is_within(&path, ".cargo")
}

/// Paths outside crate roots that no crate compiles or reads; CI checks workflows and
/// packaging runs its own tests.
fn is_rust_free(path: &str) -> bool {
    path.ends_with(".md")
        || path == "LICENSE"
        || ["docs", ".github", "packaging"]
            .iter()
            .any(|root| is_within(path, root))
}

#[cfg(test)]
mod tests {
    use super::{ExtraChecks, GoModule, Package, Selection, select_extra_checks, select_packages};

    fn workspace() -> Vec<Package> {
        vec![
            Package::new("assets", "crates/assets", &[]),
            Package::new("meshing", "crates/meshing", &["assets"]),
            Package::new("render", "crates/render", &["assets", "meshing"]),
            Package::new("bedrock-client", "app", &["render"]),
        ]
    }

    #[test]
    fn selects_owner_and_transitive_reverse_dependencies() {
        assert_eq!(
            select_packages(&["crates/assets/src/lib.rs"], &workspace()),
            Selection::Packages(vec![
                "assets".into(),
                "bedrock-client".into(),
                "meshing".into(),
                "render".into(),
            ])
        );
    }

    #[test]
    fn selects_only_the_leaf_package_for_leaf_changes() {
        assert_eq!(
            select_packages(&["app/src/main.rs"], &workspace()),
            Selection::Packages(vec!["bedrock-client".into()])
        );
    }

    #[test]
    fn builtin_hud_changes_select_the_ui_owner_and_its_consumers() {
        let packages = vec![
            Package::new("json-ui", "crates/json-ui", &[]),
            Package::new("client-ui", "crates/client-ui", &["json-ui"]),
            Package::new("bedrock-client", "app", &["client-ui"]),
            Package::new("sim", "crates/sim", &[]),
        ];
        assert_eq!(
            select_packages(&["assets/java-hud/ui/hud_screen.json"], &packages),
            Selection::Packages(vec![
                "bedrock-client".into(),
                "client-ui".into(),
                "json-ui".into(),
            ])
        );
    }

    #[test]
    fn workspace_inputs_require_the_full_gate() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
        ] {
            assert_eq!(select_packages(&[path], &workspace()), Selection::Workspace);
        }
    }

    #[test]
    fn docs_markdown_workflows_and_packaging_skip_package_compilation() {
        assert_eq!(
            select_packages(
                &[
                    "docs/decoder.md",
                    "docs/diagram.svg",
                    "README.md",
                    "THIRD_PARTY_NOTICES.md",
                    "core/README.md",
                    ".github/workflows/ci.yml",
                    "packaging/install.sh.in",
                ],
                &workspace()
            ),
            Selection::NoPackages
        );
    }

    #[test]
    fn markdown_inside_a_crate_still_selects_it() {
        assert_eq!(
            select_packages(&["app/README.md"], &workspace()),
            Selection::Packages(vec!["bedrock-client".into()])
        );
    }

    #[test]
    fn plan_and_evidence_ledger_select_the_architecture_tests() {
        for path in ["plan.md", "docs/evidence/ledger.md"] {
            assert_eq!(
                select_packages(&[path], &workspace()),
                Selection::Packages(vec!["architecture".into()])
            );
        }
    }

    #[test]
    fn go_sources_keep_the_full_gate_because_crates_read_them() {
        assert_eq!(
            select_packages(&["core/session/relay.go"], &workspace()),
            Selection::Workspace
        );
    }

    fn go_modules() -> Vec<GoModule> {
        vec![
            GoModule::new("core", true),
            GoModule::new("tools/registrygen", true),
            GoModule::new("tools/localserver", false),
        ]
    }

    #[test]
    fn go_changes_select_their_owning_module() {
        let checks = select_extra_checks(
            &[
                "core/session/relay.go",
                "tools/localserver/go.sum",
                "app/src/main.rs",
            ],
            &go_modules(),
        );
        assert_eq!(
            checks.go_modules,
            vec![
                GoModule::new("core", true),
                GoModule::new("tools/localserver", false)
            ]
        );
        assert!(!checks.packaging);
    }

    #[test]
    fn workspace_module_manifests_select_every_workspace_module() {
        for path in ["core/go.mod", "tools/registrygen/go.sum"] {
            assert_eq!(
                select_extra_checks(&[path], &go_modules()).go_modules,
                vec![
                    GoModule::new("core", true),
                    GoModule::new("tools/registrygen", true)
                ],
                "{path}"
            );
        }
        assert_eq!(
            select_extra_checks(&["tools/localserver/go.mod"], &go_modules()).go_modules,
            vec![GoModule::new("tools/localserver", false)]
        );
    }

    #[test]
    fn go_work_selects_every_workspace_module() {
        let checks = select_extra_checks(&["go.work.sum"], &go_modules());
        assert_eq!(
            checks.go_modules,
            vec![
                GoModule::new("core", true),
                GoModule::new("tools/registrygen", true)
            ]
        );
    }

    #[test]
    fn packaging_changes_select_the_packaging_tests() {
        assert_eq!(
            select_extra_checks(&["packaging/install.sh.in"], &go_modules()),
            ExtraChecks {
                go_modules: vec![],
                packaging: true
            }
        );
        assert_eq!(
            select_extra_checks(&[".github/workflows/ci.yml", "docs/a.md"], &go_modules()),
            ExtraChecks::default()
        );
    }

    #[test]
    fn unknown_paths_fail_safe_to_the_full_gate() {
        assert_eq!(
            select_packages(&["unexpected/build-input.txt"], &workspace()),
            Selection::Workspace
        );
    }

    #[test]
    fn deepest_package_root_owns_nested_workspace_paths() {
        let packages = vec![
            Package::new("outer", "tools", &[]),
            Package::new("inner", "tools/devtool", &[]),
        ];
        assert_eq!(
            select_packages(&["tools/devtool/src/main.rs"], &packages),
            Selection::Packages(vec!["inner".into()])
        );
    }
}
