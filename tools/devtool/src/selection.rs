use std::collections::{BTreeSet, VecDeque};

use workspace_declarations::within;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub(crate) name: String,
    pub(crate) cargo_id: String,
    pub(crate) root: String,
    pub(crate) dependencies: Vec<String>,
    pub(crate) doctest: bool,
    /// `[package.metadata.cinnabar] platform-specific`, which the architecture gate enforces.
    pub(crate) platforms: Vec<String>,
    /// `[package.metadata.cinnabar] reads`: other crates' files this package reads.
    pub(crate) reads: Vec<String>,
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
            platforms: Vec::new(),
            reads: Vec::new(),
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
            platforms: Vec::new(),
            reads: Vec::new(),
        }
    }

    /// Sets the platforms whose code this package contains.
    #[must_use]
    pub fn with_platforms(mut self, platforms: Vec<String>) -> Self {
        self.platforms = platforms;
        self
    }

    /// Sets the repository paths, which may use `*`, of other crates' files this package reads.
    #[must_use]
    pub fn with_reads(mut self, reads: Vec<String>) -> Self {
        self.reads = reads;
        self
    }

    /// Whether this package declares reading `path`, a file or directory it names.
    fn declares_read(&self, path: &str) -> bool {
        self.reads
            .iter()
            .any(|pattern| workspace_declarations::read_names(pattern, path))
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

/// Shared carrier identity input read by the Rust runtime and registry validators.
const TARGET_MANIFEST: &str = "assets/bedrock-target.json";

/// Selects Rust packages; paths no rule recognises fail safe to the whole workspace.
#[must_use]
pub fn select_packages(changed_paths: &[&str], packages: &[Package]) -> Selection {
    let Some(mut selected) = changed_packages(changed_paths, packages) else {
        return Selection::Workspace;
    };
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

/// Returns the packages that own or read the changed paths, before adding reverse dependencies,
/// or `None` when a path needs the whole workspace.
pub(crate) fn changed_packages(
    changed_paths: &[&str],
    packages: &[Package],
) -> Option<BTreeSet<String>> {
    if changed_paths.iter().any(|path| is_workspace_input(path)) {
        return None;
    }
    let mut selected = BTreeSet::new();
    for path in changed_paths {
        let path = normalize(path);
        let mut read = false;
        for reader in packages
            .iter()
            .filter(|package| package.declares_read(&path))
        {
            selected.insert(reader.name.clone());
            read = true;
        }
        if let Some(owner) = owner(&path, packages) {
            selected.insert(owner.name.clone());
        } else if (!read || is_go_input(&path)) && !is_rust_free(&path) {
            return None;
        }
    }
    Some(selected)
}

/// Go sources run everything even when declared: Rust tests also reach them through the Go
/// toolchain, which `reads` declarations cannot see.
fn is_go_input(path: &str) -> bool {
    path.ends_with(".go")
        || matches!(
            path.rsplit('/').next(),
            Some("go.mod" | "go.sum" | "go.work" | "go.work.sum")
        )
}

/// Returns the package with the deepest root containing a normalized `path`.
pub(crate) fn owner<'a>(path: &str, packages: &'a [Package]) -> Option<&'a Package> {
    packages
        .iter()
        .filter(|package| within(path, &package.root))
        .max_by_key(|package| package.root.len())
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
        packaging |= within(&path, "packaging");
        if path == TARGET_MANIFEST || is_registry_manifest(&path) {
            selected.extend(
                go_modules
                    .iter()
                    .filter(|module| module.dir == "tools/registrygen"),
            );
        }
        let owner = go_modules
            .iter()
            .filter(|module| module.dir.is_empty() || within(&path, &module.dir))
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

fn is_workspace_input(path: &str) -> bool {
    let path = normalize(path);
    matches!(
        path.as_str(),
        "Cargo.toml" | "Cargo.lock" | "rust-toolchain.toml" | "rust-toolchain"
    ) || within(&path, ".cargo")
}

/// Recognizes the versioned metadata files consumed only by registrygen.
fn is_registry_manifest(path: &str) -> bool {
    [
        "assets/block-projection-v",
        "assets/registry-foundation-v",
        "assets/vanilla-fallback-source-v",
    ]
    .iter()
    .any(|prefix| {
        path.strip_prefix(*prefix)
            .and_then(|suffix| suffix.strip_suffix(".json"))
            .is_some_and(|version| {
                !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
            })
    })
}

/// Generator source affects Rust only through separately selected generated artifacts.
fn is_generator_source(path: &str) -> bool {
    (within(path, "tools/registrygen") && path.ends_with(".go"))
        || matches!(
            path,
            "tools/registrygen/update_bindings.py"
                | "tools/itembindinggen/main.py"
                | "tools/itembindinggen/test_main.py"
        )
}

/// Paths outside crate roots that no crate compiles or reads; CI checks workflows and
/// packaging runs its own tests. Crate owners and external readers are selected first.
/// CI's standalone-doc classifier in `.github/workflows/ci.yml` is deliberately narrower:
/// it skips Rust, Go and Platform jobs together, so reference docs, assets, workflows,
/// packaging and unknown doc directories still run those jobs. This only selects Rust crates.
fn is_rust_free(path: &str) -> bool {
    is_registry_manifest(path)
        || is_generator_source(path)
        || path.ends_with(".md")
        || path == "LICENSE"
        || ["docs", ".github", "packaging"]
            .iter()
            .any(|root| within(path, root))
}

#[cfg(test)]
mod tests {
    use super::{
        ExtraChecks, GoModule, Package, Selection, TARGET_MANIFEST, select_extra_checks,
        select_packages,
    };

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
            Package::new("json-ui", "crates/json-ui", &[])
                .with_reads(vec!["assets/java-hud".into()]),
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
    fn target_manifest_selects_carrier_owner_and_reverse_consumers() {
        let mut packages = workspace();
        packages[0] = packages[0].clone().with_reads(vec![TARGET_MANIFEST.into()]);
        packages.push(Package::new("inventory", "crates/inventory", &[]));
        assert_eq!(
            select_packages(&[TARGET_MANIFEST], &packages),
            Selection::Packages(vec![
                "assets".into(),
                "bedrock-client".into(),
                "meshing".into(),
                "render".into()
            ])
        );
        assert_eq!(
            select_extra_checks(&[TARGET_MANIFEST], &go_modules()).go_modules,
            vec![GoModule::new("tools/registrygen", true)]
        );
    }

    #[test]
    fn registry_manifests_select_their_go_validator_without_rust_packages() {
        for path in [
            "assets/block-projection-v1.json",
            "assets/registry-foundation-v1.json",
            "assets/vanilla-fallback-source-v1.json",
        ] {
            assert_eq!(
                select_packages(&[path], &workspace()),
                Selection::NoPackages,
                "{path}"
            );
            assert_eq!(
                select_extra_checks(&[path], &go_modules()).go_modules,
                vec![GoModule::new("tools/registrygen", true)],
                "{path}"
            );
        }
    }

    #[test]
    fn generator_sources_do_not_select_unrelated_rust_packages() {
        for path in [
            "tools/registrygen/block_v1.go",
            "tools/registrygen/cmd/check/main.go",
            "tools/registrygen/internal/targetpin/light_test.go",
            "tools/registrygen/update_bindings.py",
            "tools/itembindinggen/main.py",
            "tools/itembindinggen/test_main.py",
        ] {
            assert_eq!(
                select_packages(&[path], &workspace()),
                Selection::NoPackages,
                "{path}"
            );
        }
        assert_eq!(
            select_extra_checks(
                &["tools/registrygen/internal/targetpin/light_test.go"],
                &go_modules()
            )
            .go_modules,
            vec![GoModule::new("tools/registrygen", true)]
        );
        assert_eq!(
            select_packages(
                &["tools/itembindinggen/main.py", "app/src/main.rs"],
                &workspace()
            ),
            Selection::Packages(vec!["bedrock-client".into()])
        );
    }

    #[test]
    fn unknown_generator_and_carrier_inputs_still_require_the_full_gate() {
        for path in [
            "assets/new-carrier.json",
            "assets/block-projection-vnext.json",
            "assets/registry-foundation-v1.json/child",
            "tools/registrygen/build-input.txt",
            "tools/registrygen/go.mod",
            "tools/itembindinggen/unknown.py",
        ] {
            assert_eq!(
                select_packages(&[path], &workspace()),
                Selection::Workspace,
                "{path}"
            );
        }
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
    fn plan_and_evidence_ledger_select_their_declared_reader() {
        let mut packages = workspace();
        packages.push(
            Package::new("architecture", "tools/architecture", &[])
                .with_reads(vec!["plan.md".into(), "docs/evidence".into()]),
        );
        for path in ["plan.md", "docs/evidence/ledger.md"] {
            assert_eq!(
                select_packages(&[path], &packages),
                Selection::Packages(vec!["architecture".into()])
            );
        }
        assert_eq!(
            select_packages(&["plan.md"], &workspace()),
            Selection::NoPackages
        );
    }

    #[test]
    fn declared_reads_of_go_sources_still_run_everything() {
        let mut packages = workspace();
        packages.push(Package::new("bridge", "crates/bridge", &[]).with_reads(vec!["core".into()]));
        assert_eq!(
            select_packages(&["core/session/relay.go"], &packages),
            Selection::Workspace
        );
        assert_eq!(
            select_packages(&["core/catalog/artwork_limit.txt"], &packages),
            Selection::Packages(vec!["bridge".into()])
        );
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
    fn declared_readers_and_their_dependents_are_affected() {
        let mut packages = workspace();
        packages.push(
            Package::new("pack-compiler", "crates/pack-compiler", &[]).with_reads(vec![
                "crates/protocol/data/retail_items_1_26_50.tsv".into(),
                "crates/meshing/fixtures/*".into(),
            ]),
        );
        packages.push(Package::new("protocol", "crates/protocol", &[]));
        assert_eq!(
            select_packages(
                &["crates/protocol/data/retail_items_1_26_50.tsv"],
                &packages
            ),
            Selection::Packages(vec!["pack-compiler".into(), "protocol".into()])
        );
        assert_eq!(
            select_packages(&["crates/protocol/data/other.tsv"], &packages),
            Selection::Packages(vec!["protocol".into()])
        );
        let mut readers = packages.clone();
        readers[0] = Package::new("assets", "crates/assets", &[])
            .with_reads(vec!["crates/protocol/data".into()]);
        assert_eq!(
            select_packages(&["crates/protocol/data/other.tsv"], &readers),
            Selection::Packages(vec![
                "assets".into(),
                "bedrock-client".into(),
                "meshing".into(),
                "protocol".into(),
                "render".into(),
            ])
        );
        assert_eq!(
            select_packages(&["crates/meshing/fixtures/deep/case.bin"], &packages),
            Selection::Packages(vec![
                "bedrock-client".into(),
                "meshing".into(),
                "pack-compiler".into(),
                "render".into(),
            ])
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
