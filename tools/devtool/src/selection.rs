use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub(crate) name: String,
    pub(crate) root: String,
    pub(crate) dependencies: Vec<String>,
}

impl Package {
    #[must_use]
    pub fn new(name: &str, root: &str, dependencies: &[&str]) -> Self {
        Self {
            name: name.into(),
            root: normalize(root),
            dependencies: dependencies.iter().map(|name| (*name).into()).collect(),
        }
    }

    pub(crate) fn from_owned(name: String, root: String, dependencies: Vec<String>) -> Self {
        Self {
            name,
            root: normalize(&root),
            dependencies,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Workspace,
    Packages(Vec<String>),
    NoPackages,
}

#[must_use]
pub fn select_packages(changed_paths: &[&str], packages: &[Package]) -> Selection {
    if changed_paths.iter().any(|path| is_workspace_input(path)) {
        return Selection::Workspace;
    }

    let mut selected = BTreeSet::new();
    for path in changed_paths {
        let path = normalize(path);
        if is_documentation(&path) {
            continue;
        }
        let Some(owner) = packages
            .iter()
            .filter(|package| is_within(&path, &package.root))
            .max_by_key(|package| package.root.len())
        else {
            return Selection::Workspace;
        };
        selected.insert(owner.name.clone());
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
        || is_within(&path, ".github")
}

fn is_documentation(path: &str) -> bool {
    is_within(path, "docs")
        || matches!(
            path,
            "README.md" | "AGENTS.md" | "CONTRIBUTING.md" | "LICENSE"
        )
}

#[cfg(test)]
mod tests {
    use super::{Package, Selection, select_packages};

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
    fn workspace_inputs_require_the_full_gate() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            ".cargo/config.toml",
            ".github/workflows/ci.yml",
        ] {
            assert_eq!(select_packages(&[path], &workspace()), Selection::Workspace);
        }
    }

    #[test]
    fn documentation_changes_skip_package_compilation() {
        assert_eq!(
            select_packages(&["docs/decoder.md", "README.md"], &workspace()),
            Selection::NoPackages
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
