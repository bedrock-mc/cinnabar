use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use crate::{ArchitectureError, policy::Policy, read};

pub(super) fn check_dependencies(
    root: &Path,
    policy: &Policy,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    check_workspace_members(root, policy, diagnostics)?;
    let workspace_dependencies = workspace_dependencies(root)?;
    let rule_paths = policy
        .crate_rules
        .iter()
        .map(|rule| (normal_path(&root.join(&rule.path)), rule.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    for rule in &policy.crate_rules {
        let manifest = root.join(&rule.path).join("Cargo.toml");
        let source = read(&manifest)?;
        let value =
            toml::from_str::<toml::Value>(&source).map_err(|source| ArchitectureError::Policy {
                path: manifest.clone(),
                source,
            })?;
        let dependencies = production_dependencies(
            &value,
            manifest.parent().unwrap_or(root),
            &workspace_dependencies,
        );
        for forbidden in &rule.forbidden_dependencies {
            if dependencies
                .iter()
                .any(|dependency| dependency.key == *forbidden || dependency.package == *forbidden)
            {
                diagnostics.push(format!("{}: forbidden dependency `{forbidden}`", rule.name));
            }
        }
        let allowed = rule
            .allowed_dependencies
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for dependency in dependencies {
            let Some(path) = dependency.path else {
                continue;
            };
            let Some(local_name) = rule_paths.get(&path) else {
                continue;
            };
            if !allowed.contains(local_name) {
                diagnostics.push(format!(
                    "{}: local dependency `{local_name}` is absent from the allowlist",
                    rule.name,
                ));
            }
        }
    }
    Ok(())
}

fn check_workspace_members(
    root: &Path,
    policy: &Policy,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = toml::from_str::<toml::Value>(&read(&manifest_path)?).map_err(|source| {
        ArchitectureError::Policy {
            path: manifest_path,
            source,
        }
    })?;
    let declared = policy
        .crate_rules
        .iter()
        .map(|rule| rule.path.trim_end_matches('/'))
        .collect::<BTreeSet<_>>();
    let members = manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("members"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str);
    for member in members {
        if !declared.contains(member.trim_end_matches('/')) {
            diagnostics.push(format!("workspace member `{member}` has no crate rule"));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct Dependency {
    key: String,
    package: String,
    path: Option<PathBuf>,
}

fn workspace_dependencies(root: &Path) -> Result<BTreeMap<String, Dependency>, ArchitectureError> {
    let manifest_path = root.join("Cargo.toml");
    let manifest = toml::from_str::<toml::Value>(&read(&manifest_path)?).map_err(|source| {
        ArchitectureError::Policy {
            path: manifest_path,
            source,
        }
    })?;
    let mut dependencies = Vec::new();
    append_dependency_table(
        manifest
            .get("workspace")
            .and_then(|workspace| workspace.get("dependencies")),
        root,
        &BTreeMap::new(),
        &mut dependencies,
    );
    Ok(dependencies
        .into_iter()
        .map(|dependency| (dependency.key.clone(), dependency))
        .collect())
}

fn production_dependencies(
    manifest: &toml::Value,
    crate_dir: &Path,
    workspace_dependencies: &BTreeMap<String, Dependency>,
) -> Vec<Dependency> {
    let mut dependencies = Vec::new();
    append_dependency_table(
        manifest.get("dependencies"),
        crate_dir,
        workspace_dependencies,
        &mut dependencies,
    );
    append_dependency_table(
        manifest.get("build-dependencies"),
        crate_dir,
        workspace_dependencies,
        &mut dependencies,
    );
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            append_dependency_table(
                target.get("dependencies"),
                crate_dir,
                workspace_dependencies,
                &mut dependencies,
            );
            append_dependency_table(
                target.get("build-dependencies"),
                crate_dir,
                workspace_dependencies,
                &mut dependencies,
            );
        }
    }
    dependencies
}

fn append_dependency_table(
    value: Option<&toml::Value>,
    crate_dir: &Path,
    workspace_dependencies: &BTreeMap<String, Dependency>,
    dependencies: &mut Vec<Dependency>,
) {
    let Some(table) = value.and_then(toml::Value::as_table) else {
        return;
    };
    for (key, value) in table {
        let details = value.as_table();
        if details
            .and_then(|table| table.get("workspace"))
            .and_then(toml::Value::as_bool)
            == Some(true)
        {
            if let Some(inherited) = workspace_dependencies.get(key) {
                let mut inherited = inherited.clone();
                inherited.key = key.clone();
                dependencies.push(inherited);
            }
            continue;
        }
        let package = details
            .and_then(|table| table.get("package"))
            .and_then(toml::Value::as_str)
            .unwrap_or(key)
            .to_owned();
        let path = details
            .and_then(|table| table.get("path"))
            .and_then(toml::Value::as_str)
            .map(|path| normal_path(&crate_dir.join(path)));
        dependencies.push(Dependency {
            key: key.clone(),
            package,
            path,
        });
    }
}

fn normal_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
