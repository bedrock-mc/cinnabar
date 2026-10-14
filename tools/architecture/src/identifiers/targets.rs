use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{ArchitectureError, policy::Policy, read};

/// Cargo entrypoints whose module trees determine production and development reachability.
#[derive(Default)]
pub(super) struct Targets {
    pub(super) production: BTreeSet<PathBuf>,
    pub(super) development: BTreeSet<PathBuf>,
}

/// Reads Cargo targets, including inferred binaries and build scripts, without running Cargo.
pub(super) fn production_targets(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<Targets, ArchitectureError> {
    let workspace = manifest(&root.join("Cargo.toml"))?;
    let files: Vec<_> = files
        .iter()
        .map(|path| path.canonicalize().unwrap_or_else(|_| path.clone()))
        .collect();
    let mut targets = Targets::default();
    for rule in &policy.crate_rules {
        let directory = root.join(&rule.path);
        let directory = directory.canonicalize().unwrap_or(directory);
        let path = directory.join("Cargo.toml");
        let value = manifest(&path)?;
        let Some(package) = value.get("package") else {
            continue;
        };
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or(&rule.name);
        let edition = package
            .get("edition")
            .and_then(toml::Value::as_str)
            .or_else(|| {
                package
                    .get("edition")
                    .and_then(|value| value.get("workspace"))
                    .and_then(toml::Value::as_bool)
                    .filter(|enabled| *enabled)
                    .and_then(|_| {
                        workspace
                            .get("workspace")?
                            .get("package")?
                            .get("edition")?
                            .as_str()
                    })
            })
            .unwrap_or("2015");
        let legacy = edition == "2015";
        let mut library_path = directory.join("src/lib.rs");
        if let Some(library) = value.get("lib") {
            if let Some(path) = library.get("path").and_then(toml::Value::as_str) {
                library_path = directory.join(path);
            } else if legacy && !library_path.is_file() {
                let name = library
                    .get("name")
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| name.replace('-', "_"));
                library_path = directory.join("src").join(format!("{name}.rs"));
            }
        } else if package.get("autolib").and_then(toml::Value::as_bool) == Some(false) {
            library_path = PathBuf::new();
        }
        let has_lib = library_path.is_file();
        if has_lib {
            targets
                .production
                .insert(library_path.canonicalize().unwrap_or(library_path));
        }
        let mut binaries = inferred(&directory, "src/bin", &files);
        if directory.join("src/main.rs").is_file() {
            let path = directory.join("src/main.rs");
            binaries.insert(name.to_owned(), path.canonicalize().unwrap_or(path));
        }
        let explicit = configured(
            &directory,
            &value,
            "bin",
            &mut binaries,
            legacy,
            has_lib,
            diagnostics,
        );
        for (target, features) in explicit {
            let test_only = features
                .iter()
                .any(|feature| rule.test_support_features.contains(feature));
            if test_only {
                targets.development.insert(target);
            } else {
                targets.production.insert(target);
            }
        }
        if package
            .get("autobins")
            .and_then(toml::Value::as_bool)
            .unwrap_or(!legacy || value.get("bin").is_none())
        {
            targets.production.extend(binaries.into_values());
        }
        for (kind, folder, automatic_key) in [
            ("test", "tests", "autotests"),
            ("example", "examples", "autoexamples"),
            ("bench", "benches", "autobenches"),
        ] {
            let mut inferred = inferred(&directory, folder, &files);
            targets.development.extend(
                configured(
                    &directory,
                    &value,
                    kind,
                    &mut inferred,
                    legacy,
                    has_lib,
                    diagnostics,
                )
                .into_iter()
                .map(|(path, _)| path),
            );
            if package
                .get(automatic_key)
                .and_then(toml::Value::as_bool)
                .unwrap_or(!legacy || value.get(kind).is_none())
            {
                targets.development.extend(inferred.into_values());
            }
        }
        match package.get("build") {
            Some(toml::Value::Boolean(false)) => {}
            Some(toml::Value::String(path)) => add_path(&directory, path, &mut targets.production),
            Some(toml::Value::Array(paths)) => {
                for path in paths.iter().filter_map(toml::Value::as_str) {
                    add_path(&directory, path, &mut targets.production);
                }
            }
            _ => add_path(&directory, "build.rs", &mut targets.production),
        }
    }
    Ok(targets)
}

/// Parses one manifest and preserves its location in read or syntax errors.
fn manifest(path: &Path) -> Result<toml::Value, ArchitectureError> {
    toml::from_str(&read(path)?).map_err(|source| ArchitectureError::Policy {
        path: path.to_path_buf(),
        source,
    })
}

/// Adds an existing source file using the same canonical identity as module edges.
fn add_path(directory: &Path, relative: &str, targets: &mut BTreeSet<PathBuf>) {
    let path = directory.join(relative);
    if path.is_file() {
        targets.insert(path.canonicalize().unwrap_or(path));
    }
}

/// Finds Cargo's single-file and directory/main.rs target layouts without descending further.
fn inferred(directory: &Path, folder: &str, files: &[PathBuf]) -> BTreeMap<String, PathBuf> {
    let base = directory.join(folder);
    files
        .iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(&base).ok()?;
            let components: Vec<_> = relative.components().collect();
            if components.first()?.as_os_str().to_str()?.starts_with('.') {
                return None;
            }
            let name = match components.as_slice() {
                [_] if path.extension().is_some_and(|extension| extension == "rs") => {
                    path.file_stem()?.to_str()?
                }
                [name, _] if path.file_name().is_some_and(|name| name == "main.rs") => {
                    name.as_os_str().to_str()?
                }
                _ => return None,
            };
            Some((name.to_owned(), path.clone()))
        })
        .collect()
}

/// Resolves configured target paths by explicit path or inferred name, removing replaced defaults.
fn configured(
    directory: &Path,
    manifest: &toml::Value,
    kind: &str,
    inferred: &mut BTreeMap<String, PathBuf>,
    legacy: bool,
    has_lib: bool,
    diagnostics: &mut Vec<String>,
) -> Vec<(PathBuf, Vec<String>)> {
    let mut paths = Vec::new();
    for target in manifest
        .get(kind)
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = target.get("name").and_then(toml::Value::as_str) else {
            diagnostics.push(format!(
                "{}: {kind} target needs a name",
                directory.join("Cargo.toml").display()
            ));
            continue;
        };
        let default = inferred.remove(name);
        let path = target
            .get("path")
            .and_then(toml::Value::as_str)
            .map(|path| directory.join(path))
            .or(default)
            .or_else(|| {
                legacy
                    .then(|| legacy_path(directory, kind, name, has_lib))
                    .flatten()
            });
        match path.filter(|path| path.is_file()) {
            Some(path) => {
                let path = path.canonicalize().unwrap_or(path);
                inferred.retain(|_, candidate| candidate != &path);
                let features = target
                    .get("required-features")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect();
                paths.push((path, features));
            }
            None => diagnostics.push(format!(
                "{}: cannot resolve {kind} target `{name}`",
                directory.join("Cargo.toml").display()
            )),
        }
    }
    paths
}

/// Resolves the historical 2015-edition binary and benchmark fallback paths.
fn legacy_path(directory: &Path, kind: &str, name: &str, has_lib: bool) -> Option<PathBuf> {
    let candidates = match kind {
        "bin" => {
            let mut paths = Vec::new();
            if !has_lib {
                paths.push(directory.join("src").join(format!("{name}.rs")));
            }
            paths.extend([
                directory.join("src/main.rs"),
                directory.join("src/bin/main.rs"),
            ]);
            paths
        }
        "bench" if name == "bench" => vec![directory.join("src/bench.rs")],
        _ => Vec::new(),
    };
    candidates.into_iter().find(|path| path.is_file())
}

/// Recognizes development directories even when their targets are not explicitly declared.
pub(super) fn development_path(relative: &str) -> bool {
    relative
        .split('/')
        .rev()
        .skip(1)
        .any(|directory| matches!(directory, "examples" | "benches"))
}
