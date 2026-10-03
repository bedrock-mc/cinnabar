use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use crate::{
    ArchitectureError,
    paths::{is_vendored, relative_slash},
    policy::Policy,
    read,
};

pub(super) fn check_dependencies(
    root: &Path,
    policy: &Policy,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    check_workspace_members(root, policy, diagnostics)?;
    let workspace_dependencies = workspace_dependencies(root)?;
    let mut rule_paths = policy
        .crate_rules
        .iter()
        .map(|rule| (normal_path(&root.join(&rule.path)), rule.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut graph = BTreeMap::new();
    for rule in &policy.crate_rules {
        let manifest = root.join(&rule.path).join("Cargo.toml");
        let source = read(&manifest)?;
        let value =
            toml::from_str::<toml::Value>(&source).map_err(|source| ArchitectureError::Policy {
                path: manifest.clone(),
                source,
            })?;
        let production = manifest_dependencies(
            &value,
            manifest.parent().unwrap_or(root),
            &workspace_dependencies,
            &["dependencies", "build-dependencies"],
        );
        let mut all = production.clone();
        all.extend(manifest_dependencies(
            &value,
            manifest.parent().unwrap_or(root),
            &workspace_dependencies,
            &["dev-dependencies"],
        ));
        graph.insert(rule.name.clone(), CrateDependencies { production, all });
    }
    let declared_paths = rule_paths.clone();
    load_vendored_dependencies(
        root,
        policy,
        &workspace_dependencies,
        &mut graph,
        &mut rule_paths,
        diagnostics,
    )?;
    for rule in &policy.crate_rules {
        let dependencies = &graph[rule.name.as_str()].production;
        if rule.dependency_free {
            for dependency in &graph[rule.name.as_str()].all {
                diagnostics.push(format!(
                    "{}: dependency-free crate has dependency `{}`",
                    rule.name, dependency.package,
                ));
            }
        }
        if !rule.forbidden_transitive_dependencies.is_empty() {
            check_transitive_dependencies(
                &rule.name,
                &rule.forbidden_transitive_dependencies,
                &graph,
                &rule_paths,
                diagnostics,
            );
        }
        for forbidden in &rule.forbidden_dependencies {
            if dependencies.iter().any(|dependency| {
                forbidden_package(forbidden, &dependency.key)
                    || forbidden_package(forbidden, &dependency.package)
            }) {
                diagnostics.push(format!("{}: forbidden dependency `{forbidden}`", rule.name));
            }
        }
        let allowed = rule
            .allowed_dependencies
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for dependency in dependencies {
            let Some(path) = &dependency.path else {
                continue;
            };
            let Some(local_name) = declared_paths.get(path) else {
                continue;
            };
            if !allowed.contains(local_name.as_str()) {
                diagnostics.push(format!(
                    "{}: local dependency `{local_name}` is absent from the allowlist",
                    rule.name,
                ));
            }
        }
    }
    Ok(())
}

/// Matches exact package names or an explicitly configured trailing-star prefix.
fn forbidden_package(pattern: &str, package: &str) -> bool {
    pattern
        .strip_suffix('*')
        .map_or(package == pattern, |prefix| package.starts_with(prefix))
}

struct CrateDependencies {
    production: Vec<Dependency>,
    all: Vec<Dependency>,
}

/// Follow local build edges and reject forbidden packages at any depth.
fn check_transitive_dependencies(
    origin: &str,
    forbidden: &[String],
    graph: &BTreeMap<String, CrateDependencies>,
    rule_paths: &BTreeMap<PathBuf, String>,
    diagnostics: &mut Vec<String>,
) {
    let mut pending = vec![(origin.to_owned(), vec![origin.to_owned()])];
    let mut visited = BTreeSet::new();
    while let Some((name, chain)) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let dependencies = &graph[&name];
        // Cargo builds the origin's tests, but does not inherit dependencies' tests.
        let dependencies = if name == origin {
            &dependencies.all
        } else {
            &dependencies.production
        };
        for dependency in dependencies {
            let local_name = dependency
                .path
                .as_ref()
                .and_then(|path| rule_paths.get(path))
                .cloned();
            let mut dependency_chain = chain.clone();
            dependency_chain.push(
                local_name
                    .clone()
                    .unwrap_or_else(|| dependency.package.clone()),
            );
            let path = dependency_chain.join(" -> ");
            if forbidden.iter().any(|forbidden| {
                forbidden_package(forbidden, &dependency.key)
                    || forbidden_package(forbidden, &dependency.package)
                    || local_name
                        .as_ref()
                        .is_some_and(|name| forbidden_package(forbidden, name))
            }) {
                diagnostics.push(format!("{origin}: forbidden dependency path `{path}`"));
            }
            if let Some(local_name) = local_name {
                pending.push((local_name, dependency_chain));
            } else if dependency.path.is_some() {
                diagnostics.push(format!(
                    "{origin}: dependency path `{path}` has no crate rule; cannot verify boundary",
                ));
            }
        }
    }
}

/// Loads declared vendored path dependencies so transitive bans also cover their manifests.
fn load_vendored_dependencies(
    root: &Path,
    policy: &Policy,
    workspace: &BTreeMap<String, Dependency>,
    graph: &mut BTreeMap<String, CrateDependencies>,
    paths: &mut BTreeMap<PathBuf, String>,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let mut pending: Vec<_> = graph
        .values()
        .flat_map(|entry| &entry.all)
        .filter_map(|dependency| dependency.path.clone())
        .collect();
    let mut visited = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if paths.contains_key(&path)
            || !visited.insert(path.clone())
            || !is_vendored(&relative_slash(root, &path), policy)
        {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        let source = read(&manifest)?;
        let value: toml::Value =
            toml::from_str(&source).map_err(|source| ArchitectureError::Policy {
                path: manifest,
                source,
            })?;
        let name = value
            .get("package")
            .and_then(|package| package.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .to_owned();
        if name.is_empty() || graph.contains_key(&name) {
            diagnostics.push(format!(
                "{}: missing or duplicate local package name; cannot verify boundary",
                relative_slash(root, &path)
            ));
            continue;
        }
        let production = manifest_dependencies(
            &value,
            &path,
            workspace,
            &["dependencies", "build-dependencies"],
        );
        let mut all = production.clone();
        all.extend(manifest_dependencies(
            &value,
            &path,
            workspace,
            &["dev-dependencies"],
        ));
        pending.extend(all.iter().filter_map(|dependency| dependency.path.clone()));
        paths.insert(path, name.clone());
        graph.insert(name, CrateDependencies { production, all });
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

/// Read the selected dependency kinds from the manifest and every target table.
fn manifest_dependencies(
    manifest: &toml::Value,
    crate_dir: &Path,
    workspace_dependencies: &BTreeMap<String, Dependency>,
    kinds: &[&str],
) -> Vec<Dependency> {
    let mut dependencies = Vec::new();
    let targets = manifest
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flat_map(|targets| targets.values());
    for table in std::iter::once(manifest).chain(targets) {
        for kind in kinds {
            append_dependency_table(
                table.get(*kind),
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
