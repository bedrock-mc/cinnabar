use std::path::Path;

use super::Policy;

pub(super) fn ignored_directory(relative: &str) -> bool {
    relative
        .split('/')
        .any(|part| matches!(part, ".git" | ".worktrees" | "target"))
}

pub(super) fn is_vendored(relative: &str, policy: &Policy) -> bool {
    policy.vendored_paths.iter().any(|prefix| {
        let prefix = prefix.trim_start_matches("./").trim_end_matches('/');
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
