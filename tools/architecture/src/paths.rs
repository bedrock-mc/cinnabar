use std::path::Path;

use workspace_declarations::{TABLE, within};

use super::Policy;

pub(super) fn ignored_directory(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| matches!(part, ".git" | ".local" | ".worktrees" | "build" | "target"))
}

pub(super) fn is_vendored(relative: &str, policy: &Policy) -> bool {
    policy.vendored.iter().any(|rule| {
        let prefix = rule.path.trim_start_matches("./").trim_end_matches('/');
        relative == prefix || relative.starts_with(&format!("{prefix}/"))
    })
}

pub(super) fn marker_literals(source: &str) -> Vec<String> {
    let mut markers = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find("RUST_MCBE_") {
        rest = &rest[index..];
        let length = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
            .count();
        markers.push(rest[..length].to_owned());
        rest = &rest[length..];
    }
    markers
}

pub(super) fn relative_slash(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Each crate rule's repository-relative root, `""` for the repository root, with its name.
pub(super) fn crate_roots(policy: &Policy) -> Vec<(&str, &str)> {
    policy
        .crate_rules
        .iter()
        .map(|rule| {
            let path = rule.path.trim_start_matches("./").trim_end_matches('/');
            (if path == "." { "" } else { path }, rule.name.as_str())
        })
        .collect()
}

/// Returns the crate with the deepest root containing the repository-relative `path`.
pub(super) fn owning_crate<'a>(
    roots: &[(&'a str, &'a str)],
    path: &str,
) -> Option<(&'a str, &'a str)> {
    roots
        .iter()
        .filter(|(root, _)| root.is_empty() || within(path, root))
        .max_by_key(|(root, _)| root.len())
        .copied()
}

/// Reads the declaration array `key` from a manifest's `[package.metadata.cinnabar]`.
pub(super) fn cinnabar_list<'a>(
    manifest: &'a toml::Value,
    key: &str,
) -> impl Iterator<Item = &'a toml::Value> {
    manifest
        .get("package")
        .and_then(|package| package.get("metadata"))
        .and_then(|metadata| metadata.get(TABLE))
        .and_then(|table| table.get(key))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
}
