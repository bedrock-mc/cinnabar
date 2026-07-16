use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Deserialize;

mod paths;

use paths::{ignored_directory, is_vendored, marker_literals, relative_slash};

#[derive(Debug, thiserror::Error)]
pub enum ArchitectureError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Policy {
        path: PathBuf,
        source: toml::de::Error,
    },
}

#[derive(Debug, Deserialize)]
struct Policy {
    production_rust_max: usize,
    module_root_max: usize,
    powershell_max: usize,
    test_max: usize,
    #[serde(default)]
    vendored_paths: Vec<String>,
    #[serde(default)]
    forbidden_artifacts: Vec<String>,
    #[serde(default, rename = "crates")]
    crate_rules: Vec<CrateRule>,
    #[serde(default)]
    markers: Vec<MarkerRule>,
}

#[derive(Debug, Deserialize)]
struct CrateRule {
    name: String,
    path: String,
    #[serde(default)]
    allowed_dependencies: Vec<String>,
    #[serde(default)]
    forbidden_dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MarkerRule {
    literal: String,
    kind: MarkerKind,
    producer: String,
    consumer: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum MarkerKind {
    Parsed,
    LogOnly,
}

pub fn check_repository(root: &Path, policy_path: &Path) -> Result<Vec<String>, ArchitectureError> {
    let policy_text = read(policy_path)?;
    let policy =
        toml::from_str::<Policy>(&policy_text).map_err(|source| ArchitectureError::Policy {
            path: policy_path.to_path_buf(),
            source,
        })?;
    let mut files = Vec::new();
    collect_files(root, root, &policy, &mut files)?;
    files.sort();

    let mut diagnostics = Vec::new();
    check_artifacts(root, &policy, &files, &mut diagnostics);
    check_sources(root, &policy, &files, &mut diagnostics)?;
    check_dependencies(root, &policy, &mut diagnostics)?;
    check_markers(root, &policy, &files, &mut diagnostics)?;
    diagnostics.sort();
    diagnostics.dedup();
    Ok(diagnostics)
}

fn check_artifacts(root: &Path, policy: &Policy, files: &[PathBuf], diagnostics: &mut Vec<String>) {
    for path in files {
        let relative = relative_slash(root, path);
        for pattern in &policy.forbidden_artifacts {
            if wildcard_match(pattern.as_bytes(), relative.as_bytes()) {
                diagnostics.push(format!(
                    "{relative}: matches forbidden artifact pattern `{pattern}`"
                ));
            }
        }
    }
}

fn wildcard_match(pattern: &[u8], value: &[u8]) -> bool {
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star_index, mut star_value_index) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len() && pattern[pattern_index] == value[value_index] {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
                pattern_index += 1;
            }
            star_index = Some(pattern_index);
            star_value_index = value_index;
        } else if let Some(after_star) = star_index {
            star_value_index += 1;
            value_index = star_value_index;
            pattern_index = after_star;
        } else {
            return false;
        }
    }
    pattern[pattern_index..].iter().all(|byte| *byte == b'*')
}

fn read(path: &Path) -> Result<String, ArchitectureError> {
    fs::read_to_string(path).map_err(|source| ArchitectureError::Read {
        path: path.to_path_buf(),
        source,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    policy: &Policy,
    files: &mut Vec<PathBuf>,
) -> Result<(), ArchitectureError> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| ArchitectureError::Read {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| ArchitectureError::Read {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let relative = relative_slash(root, &path);
        if entry
            .file_type()
            .map_err(|source| ArchitectureError::Read {
                path: path.clone(),
                source,
            })?
            .is_dir()
        {
            if ignored_directory(&relative) || is_vendored(&relative, policy) {
                continue;
            }
            collect_files(root, &path, policy, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn check_sources(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    for path in files {
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("rs" | "ps1")) {
            continue;
        }
        let relative = relative_slash(root, path);
        let source = read(path)?;
        let lines = source.lines().count();
        if extension == Some("ps1") {
            if lines > policy.powershell_max {
                diagnostics.push(format!(
                    "{relative}: {lines} lines exceeds PowerShell limit {}",
                    policy.powershell_max
                ));
            }
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let is_test = path
            .components()
            .any(|part| matches!(part, Component::Normal(value) if value == "tests"));
        let (limit, label) = if is_test {
            (policy.test_max, "test")
        } else if matches!(file_name, "lib.rs" | "main.rs" | "mod.rs") {
            (policy.module_root_max, "module-root")
        } else {
            (policy.production_rust_max, "production Rust")
        };
        if lines > limit {
            diagnostics.push(format!(
                "{relative}: {lines} lines exceeds {label} limit {limit}"
            ));
        }
        for (index, line) in source.lines().enumerate() {
            let compact = line.split_whitespace().collect::<String>();
            if compact.starts_with("pubuse") && compact.contains("::*") {
                diagnostics.push(format!(
                    "{relative}:{}: glob re-export is forbidden",
                    index + 1
                ));
            }
            if line.contains("pub fn ") && line.contains("_for_test") {
                diagnostics.push(format!(
                    "{relative}:{}: test-only public API is forbidden",
                    index + 1
                ));
            }
        }
    }
    Ok(())
}

fn check_dependencies(
    root: &Path,
    policy: &Policy,
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
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
        let dependencies = production_dependencies(&value, manifest.parent().unwrap_or(root));
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

struct Dependency {
    key: String,
    package: String,
    path: Option<PathBuf>,
}

fn production_dependencies(manifest: &toml::Value, crate_dir: &Path) -> Vec<Dependency> {
    let mut dependencies = Vec::new();
    append_dependency_table(manifest.get("dependencies"), crate_dir, &mut dependencies);
    append_dependency_table(
        manifest.get("build-dependencies"),
        crate_dir,
        &mut dependencies,
    );
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values() {
            append_dependency_table(target.get("dependencies"), crate_dir, &mut dependencies);
            append_dependency_table(
                target.get("build-dependencies"),
                crate_dir,
                &mut dependencies,
            );
        }
    }
    dependencies
}

fn append_dependency_table(
    value: Option<&toml::Value>,
    crate_dir: &Path,
    dependencies: &mut Vec<Dependency>,
) {
    let Some(table) = value.and_then(toml::Value::as_table) else {
        return;
    };
    for (key, value) in table {
        let details = value.as_table();
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

fn check_markers(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let mut occurrences = BTreeMap::<String, Vec<(String, usize)>>::new();
    for path in files {
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "ps1")
        ) {
            continue;
        }
        let relative = relative_slash(root, path);
        let source = read(path)?;
        for marker in marker_literals(&source) {
            occurrences
                .entry(marker)
                .or_default()
                .push((relative.clone(), 1));
        }
    }
    let declared = policy
        .markers
        .iter()
        .map(|rule| rule.literal.as_str())
        .collect::<BTreeSet<_>>();
    for marker in occurrences.keys() {
        if !declared.contains(marker.as_str()) {
            diagnostics.push(format!("marker `{marker}` has no declared expectation"));
        }
    }
    for rule in &policy.markers {
        let entries = occurrences.get(&rule.literal).cloned().unwrap_or_default();
        let producer_count = entries
            .iter()
            .filter(|(path, _)| path == &rule.producer)
            .count();
        if producer_count != 1 {
            diagnostics.push(format!(
                "marker `{}` producer count {producer_count}, expected 1 in {}",
                rule.literal, rule.producer
            ));
        }
        if rule.kind == MarkerKind::Parsed {
            let consumer_count = rule
                .consumer
                .as_ref()
                .map(|consumer| entries.iter().filter(|(path, _)| path == consumer).count())
                .unwrap_or(0);
            if consumer_count != 1 {
                diagnostics.push(format!(
                    "marker `{}` consumer count {consumer_count}, expected 1 in {}",
                    rule.literal,
                    rule.consumer
                        .as_deref()
                        .unwrap_or("<missing policy consumer>")
                ));
            }
        }
    }
    Ok(())
}
