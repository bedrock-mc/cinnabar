use std::{collections::BTreeSet, path::PathBuf};

use serde::Deserialize;

use crate::{DevtoolError, Package};

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
            Ok(Package::from_owned(
                package.name,
                root.to_string_lossy().replace('\\', "/"),
                dependencies,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::packages_from_metadata;
    use crate::{Selection, select_packages};

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
                    "dependencies": []
                },
                {
                    "id": "render 0.1.0 (path+file:///repo/crates/render)",
                    "name": "render",
                    "manifest_path": "/repo/crates/render/Cargo.toml",
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
}
