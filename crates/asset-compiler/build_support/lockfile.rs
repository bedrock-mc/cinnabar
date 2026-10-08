//! The registry and git packages a workspace package reaches through `Cargo.lock`, so a dependency
//! update that can change compiler output changes the compiler digest while unrelated ones don't.

use std::collections::BTreeSet;

#[derive(Debug, Default)]
struct Package {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
    dependencies: Vec<String>,
}

/// `name version source checksum` lines for every external package `root` reaches, sorted.
pub fn external_closure(lockfile: &str, root: &str) -> Vec<String> {
    let packages = parse(lockfile);
    let mut seen = BTreeSet::new();
    let mut stack: Vec<usize> = packages
        .iter()
        .position(|package| package.name == root)
        .into_iter()
        .collect();
    while let Some(index) = stack.pop() {
        if !seen.insert(index) {
            continue;
        }
        for dependency in &packages[index].dependencies {
            stack.extend(resolve(&packages, dependency));
        }
    }
    let mut lines: Vec<String> = seen
        .into_iter()
        .map(|index| &packages[index])
        .filter_map(|package| {
            let source = package.source.as_deref()?;
            Some(format!(
                "{} {} {source} {}",
                package.name,
                package.version,
                package.checksum.as_deref().unwrap_or("-")
            ))
        })
        .collect();
    lines.sort();
    lines
}

/// A `dependencies` entry is `name`, `name version` or `name version (source)`.
fn resolve(packages: &[Package], entry: &str) -> Option<usize> {
    let mut parts = entry.splitn(3, ' ');
    let name = parts.next()?;
    let version = parts.next();
    let source = parts
        .next()
        .map(|source| source.trim_start_matches('(').trim_end_matches(')'));
    packages.iter().position(|package| {
        package.name == name
            && version.is_none_or(|version| package.version == version)
            && source.is_none_or(|source| package.source.as_deref() == Some(source))
    })
}

fn parse(lockfile: &str) -> Vec<Package> {
    let mut packages = Vec::new();
    let mut current: Option<Package> = None;
    let mut in_dependencies = false;
    for line in lockfile.lines().map(str::trim) {
        if line == "[[package]]" {
            packages.extend(current.take());
            current = Some(Package::default());
            in_dependencies = false;
            continue;
        }
        if line.starts_with('[') {
            packages.extend(current.take());
            in_dependencies = false;
            continue;
        }
        let Some(package) = current.as_mut() else {
            continue;
        };
        if in_dependencies {
            if line.starts_with(']') {
                in_dependencies = false;
            } else if let Some(entry) = quoted(line) {
                package.dependencies.push(entry.to_owned());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        match key {
            "name" => package.name = quoted(value).unwrap_or_default().to_owned(),
            "version" => package.version = quoted(value).unwrap_or_default().to_owned(),
            "source" => package.source = quoted(value).map(str::to_owned),
            "checksum" => package.checksum = quoted(value).map(str::to_owned),
            "dependencies" => {
                let inline = value.trim();
                if inline.ends_with(']') {
                    package.dependencies.extend(
                        inline
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .filter_map(quoted)
                            .map(str::to_owned),
                    );
                } else {
                    in_dependencies = true;
                }
            }
            _ => {}
        }
    }
    packages.extend(current);
    packages
}

fn quoted(text: &str) -> Option<&str> {
    let text = text.trim().trim_end_matches(',');
    text.strip_prefix('"')?.strip_suffix('"')
}

#[cfg(test)]
mod tests {
    use super::external_closure;

    const LOCKFILE: &str = r#"
version = 4

[[package]]
name = "asset-compiler"
version = "0.1.5"
dependencies = [
 "assets",
 "image",
]

[[package]]
name = "assets"
version = "0.1.5"
dependencies = ["thiserror 2.0.18"]

[[package]]
name = "image"
version = "0.25.6"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aa"
dependencies = [
 "png",
]

[[package]]
name = "png"
version = "0.17.16"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bb"

[[package]]
name = "thiserror"
version = "1.0.69"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "cc"

[[package]]
name = "thiserror"
version = "2.0.18"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "dd"

[[package]]
name = "bevy"
version = "0.16.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "ee"
"#;

    #[test]
    fn the_closure_follows_dependency_lists_and_keeps_only_external_packages() {
        let registry = "registry+https://github.com/rust-lang/crates.io-index";
        assert_eq!(
            external_closure(LOCKFILE, "asset-compiler"),
            [
                format!("image 0.25.6 {registry} aa"),
                format!("png 0.17.16 {registry} bb"),
                format!("thiserror 2.0.18 {registry} dd"),
            ]
        );
    }

    #[test]
    fn an_unrelated_dependency_update_leaves_the_closure_unchanged() {
        let updated = LOCKFILE
            .replace("0.16.0", "0.17.0")
            .replace("\"ee\"", "\"ff\"");
        assert_eq!(
            external_closure(&updated, "asset-compiler"),
            external_closure(LOCKFILE, "asset-compiler")
        );
        let bumped = LOCKFILE.replace("\"bb\"", "\"b2\"");
        assert_ne!(
            external_closure(&bumped, "asset-compiler"),
            external_closure(LOCKFILE, "asset-compiler")
        );
    }

    #[test]
    fn the_workspace_lockfile_reaches_the_image_decoder_but_not_the_engine() {
        // Read at run time: an embedded lockfile would enter the compiler digest whole.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock");
        let lockfile = std::fs::read_to_string(path).unwrap();
        let closure = external_closure(&lockfile, "asset-compiler");
        assert!(closure.iter().any(|line| line.starts_with("image ")));
        assert!(!closure.iter().any(|line| line.starts_with("bevy ")));
    }
}
