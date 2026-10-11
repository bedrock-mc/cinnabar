use std::{collections::BTreeSet, path::PathBuf};

use serde::Deserialize;
use workspace_declarations::{PLATFORM_SPECIFIC, READS, TABLE};

use crate::{DevtoolError, GoModule, Package, selection::normalize};

#[derive(Deserialize)]
struct Metadata {
    workspace_root: PathBuf,
    workspace_members: BTreeSet<String>,
    packages: Vec<MetadataPackage>,
}

#[derive(Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    manifest_path: PathBuf,
    dependencies: Vec<MetadataDependency>,
    targets: Vec<MetadataTarget>,
    #[serde(default)]
    metadata: serde_json::Value,
}

/// Reads the declaration array `key` from a package's `[package.metadata.cinnabar]`.
fn declarations(metadata: &serde_json::Value, key: &str) -> Vec<String> {
    metadata
        .get(TABLE)
        .and_then(|table| table.get(key))
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[derive(Deserialize)]
struct MetadataTarget {
    doctest: bool,
}

#[derive(Deserialize)]
struct MetadataDependency {
    name: String,
    path: Option<PathBuf>,
}

pub fn packages_from_metadata(json: &str) -> Result<Vec<Package>, DevtoolError> {
    let metadata: Metadata = serde_json::from_str(json)?;
    let workspace_roots = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .filter_map(|package| package.manifest_path.parent().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    metadata
        .packages
        .into_iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .map(|package| {
            let root = package
                .manifest_path
                .parent()
                .ok_or_else(|| DevtoolError::ManifestWithoutParent(package.manifest_path.clone()))?
                .strip_prefix(&metadata.workspace_root)
                .map_err(|_| DevtoolError::ManifestOutsideWorkspace {
                    manifest: package.manifest_path.clone(),
                    root: metadata.workspace_root.clone(),
                })?;
            let dependencies = package
                .dependencies
                .into_iter()
                .filter(|dependency| {
                    dependency
                        .path
                        .as_deref()
                        .is_some_and(|path| workspace_roots.contains(path))
                })
                .map(|dependency| dependency.name)
                .collect();
            let platforms = declarations(&package.metadata, PLATFORM_SPECIFIC);
            let reads = declarations(&package.metadata, READS);
            Ok(Package::from_owned(
                package.id,
                package.name,
                root.to_string_lossy().replace('\\', "/"),
                dependencies,
                package.targets.iter().any(|target| target.doctest),
            )
            .with_platforms(platforms)
            .with_reads(reads))
        })
        .collect()
}

/// Returns the module of each `go.mod` path, marking those `go.work` lists.
#[must_use]
pub fn go_modules(go_mod_paths: &[&str], go_work: Option<&str>) -> Vec<GoModule> {
    let workspace = go_work.map(go_work_uses).unwrap_or_default();
    let mut modules: Vec<_> = go_mod_paths
        .iter()
        .filter_map(|path| {
            let path = normalize(path);
            let dir = if path == "go.mod" {
                ""
            } else {
                path.strip_suffix("/go.mod")?
            };
            Some(GoModule::new(dir, workspace.contains(dir)))
        })
        .collect();
    modules.sort();
    modules
}

/// Module directories named by `use` directives, single or block form.
fn go_work_uses(go_work: &str) -> BTreeSet<String> {
    let mut uses = BTreeSet::new();
    let mut in_block = false;
    for line in go_work.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        let entry = if in_block {
            if line == ")" {
                in_block = false;
                continue;
            }
            line
        } else if let Some(rest) = line.strip_prefix("use") {
            let rest = rest.trim();
            if rest == "(" {
                in_block = true;
                continue;
            }
            rest
        } else {
            continue;
        };
        let dir = normalize(entry.trim_matches('"'));
        if !dir.is_empty() {
            uses.insert(if dir == "." { String::new() } else { dir });
        }
    }
    uses
}

#[cfg(test)]
mod tests {
    use super::{go_modules, packages_from_metadata};
    use crate::{GoModule, Selection, TestRunner, select_packages, verification_commands};

    #[test]
    fn go_work_marks_its_modules_and_leaves_others_standalone() {
        let go_work = "go 1.26.1\n\nuse (\n\t./core // client core\n\t\"./tools/registrygen\"\n)\nuse ./tools/fixturegen\n";
        assert_eq!(
            go_modules(
                &[
                    "tools/localserver/go.mod",
                    "core/go.mod",
                    "tools/registrygen/go.mod",
                    "tools/fixturegen/go.mod",
                ],
                Some(go_work)
            ),
            vec![
                GoModule::new("core", true),
                GoModule::new("tools/fixturegen", true),
                GoModule::new("tools/localserver", false),
                GoModule::new("tools/registrygen", true),
            ]
        );
        assert_eq!(
            go_modules(&["core/go.mod"], None),
            vec![GoModule::new("core", false)]
        );
    }

    /// The nextest supplement honors Cargo target metadata and the affected-package selection.
    #[test]
    fn doctest_commands_respect_target_metadata_and_selection() {
        let packages = [
            ("enabled", "lib", true),
            ("disabled", "lib", false),
            ("binary", "bin", false),
            ("other", "lib", true),
        ];
        let metadata = serde_json::json!({
            "workspace_root": "/repo",
            "workspace_members": packages.map(|(name, _, _)| name),
            "packages": packages.map(|(name, kind, doctest)| serde_json::json!({
                "id": name,
                "name": name,
                "manifest_path": format!("/repo/{name}/Cargo.toml"),
                "dependencies": [],
                "targets": [{"kind": [kind], "doctest": doctest}],
            })),
        });
        let packages = packages_from_metadata(&metadata.to_string()).unwrap();
        for (selection, expected) in [
            (
                Selection::Workspace,
                Some("cargo test --doc --locked -p enabled -p other"),
            ),
            (
                Selection::Packages(vec!["enabled".into(), "disabled".into(), "binary".into()]),
                Some("cargo test --doc --locked -p enabled"),
            ),
            (
                Selection::Packages(vec!["disabled".into(), "binary".into()]),
                None,
            ),
            (Selection::NoPackages, None),
        ] {
            let commands = verification_commands(&selection, TestRunner::Nextest, &packages);
            let doctests: Vec<_> = commands
                .iter()
                .filter(|command| command.args.iter().any(|arg| arg == "--doc"))
                .map(ToString::to_string)
                .collect();
            assert_eq!(
                doctests,
                expected.into_iter().map(str::to_owned).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn platform_and_read_declarations_come_from_package_metadata() {
        let metadata = serde_json::json!({
            "workspace_root": "/repo",
            "workspace_members": ["portable", "native"],
            "packages": [
                {"id": "portable", "name": "portable", "manifest_path": "/repo/portable/Cargo.toml", "dependencies": [], "targets": [], "metadata": null},
                {"id": "native", "name": "native", "manifest_path": "/repo/native/Cargo.toml", "dependencies": [], "targets": [],
                 "metadata": {"cinnabar": {"platform-specific": ["macos", "windows"], "reads": ["crates/portable/data/*"]}, "other": {}}},
            ],
        });
        let packages = packages_from_metadata(&metadata.to_string()).unwrap();
        assert!(packages[0].platforms.is_empty());
        assert_eq!(packages[1].platforms, ["macos", "windows"]);
        assert_eq!(packages[1].reads, ["crates/portable/data/*"]);
    }

    #[test]
    fn cargo_metadata_becomes_workspace_path_dependencies() {
        let metadata = r#"{
            "workspace_root": "/repo",
            "workspace_members": ["assets 0.1.0 (path+file:///repo/crates/assets)", "render 0.1.0 (path+file:///repo/crates/render)"],
            "packages": [
                {
                    "id": "assets 0.1.0 (path+file:///repo/crates/assets)",
                    "name": "assets",
                    "manifest_path": "/repo/crates/assets/Cargo.toml",
                    "targets": [{"doctest": true}],
                    "dependencies": []
                },
                {
                    "id": "render 0.1.0 (path+file:///repo/crates/render)",
                    "name": "render",
                    "manifest_path": "/repo/crates/render/Cargo.toml",
                    "targets": [{"doctest": true}],
                    "dependencies": [{"name": "assets", "path": "/repo/crates/assets"}]
                }
            ]
        }"#;
        let packages = packages_from_metadata(metadata).expect("parse metadata");
        assert_eq!(
            select_packages(&["crates/assets/src/lib.rs"], &packages),
            Selection::Packages(vec!["assets".into(), "render".into()])
        );
    }

    #[test]
    fn a_registry_name_collision_keeps_all_filters_on_the_workspace_package() {
        let workspace_id = "path+file:///repo/crates/inventory#inventory@0.1.0";
        let registry_id = "registry+https://github.com/rust-lang/crates.io-index#inventory@0.3.24";
        let metadata = serde_json::json!({
            "workspace_root": "/repo",
            "workspace_members": [workspace_id],
            "packages": [
                {"id": workspace_id, "name": "inventory", "manifest_path": "/repo/crates/inventory/Cargo.toml", "dependencies": [], "targets": [{"doctest": true}]},
                {"id": registry_id, "name": "inventory", "manifest_path": "/registry/inventory/Cargo.toml", "dependencies": [], "targets": [{"doctest": true}]},
            ],
        });
        let packages = packages_from_metadata(&metadata.to_string()).unwrap();
        assert_eq!(packages.len(), 1);
        let affected = select_packages(&["crates/inventory/src/lib.rs"], &packages);
        assert_eq!(affected, Selection::Packages(vec!["inventory".into()]));
        for selection in [affected, Selection::Workspace] {
            for runner in [TestRunner::Cargo, TestRunner::Nextest] {
                let commands = verification_commands(&selection, runner, &packages);
                for command in commands.iter().skip(2) {
                    for filter in command.args.windows(2).filter(|pair| pair[0] == "-p") {
                        assert_eq!(filter[1], workspace_id, "{command}");
                    }
                }
                if matches!(selection, Selection::Packages(_)) {
                    for command in commands.iter().skip(2) {
                        assert!(
                            command.args.iter().any(|arg| arg == workspace_id),
                            "{command}"
                        );
                    }
                }
            }
        }
    }
}
