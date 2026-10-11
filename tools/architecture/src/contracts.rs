//! Keeps shared ECS hubs limited to small data contracts.
use std::path::{Path, PathBuf};

use syn::Item;

use crate::{ArchitectureError, paths::relative_slash, policy::Policy, read};

/// Enforces each hub's source budget and rejects executable declarations.
pub(super) fn check_contracts(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    for rule in &policy.crate_rules {
        if !rule.contract_only && rule.max_rust_lines.is_none() {
            continue;
        }
        let prefix = format!("{}/src/", rule.path);
        let mut lines = 0;
        for path in files {
            let relative = relative_slash(root, path);
            if !relative.starts_with(&prefix)
                || path.extension().is_none_or(|extension| extension != "rs")
            {
                continue;
            }
            let source = read(path)?;
            lines += source.lines().count();
            if rule.contract_only {
                match syn::parse_file(&source) {
                    Ok(parsed) => check_items(&parsed.items, &relative, diagnostics),
                    Err(error) => diagnostics
                        .push(format!("{relative}: cannot parse contract source: {error}")),
                }
            }
        }
        if let Some(limit) = rule.max_rust_lines
            && lines > limit
        {
            diagnostics.push(format!(
                "{}: {lines} Rust source lines exceeds crate limit {limit}",
                rule.path
            ));
        }
    }
    Ok(())
}

/// Allows data declarations and module wiring, including nested modules.
fn check_items(items: &[Item], path: &str, diagnostics: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Struct(_) | Item::Enum(_) | Item::Use(_) => {}
            Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    check_items(items, path, diagnostics);
                }
            }
            _ => diagnostics.push(format!(
                "{path}: contract-only crates may contain only structs, enums, imports and modules"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks an isolated source file using the hub's declaration rules.
    fn failures(source: &str) -> Vec<String> {
        let file = syn::parse_file(source).unwrap();
        let mut diagnostics = Vec::new();
        check_items(&file.items, "contract.rs", &mut diagnostics);
        diagnostics
    }

    #[test]
    fn contracts_allow_resource_wrappers_sets_and_observations() {
        assert!(
            failures(
                "use bevy::prelude::*;
             #[derive(Resource, Deref)] pub struct Player(pub domain::Player);
             #[derive(SystemSet)] pub enum Frame { Input, Physics }
             pub struct Focus { pub absorbs_input: bool }
             mod nested { pub struct Observation(pub bool); }
             mod external;"
            )
            .is_empty()
        );
    }

    #[test]
    fn contracts_reject_systems_methods_and_macro_generated_logic() {
        for source in [
            "fn system() {}",
            "struct State; impl State { fn advance(&mut self) {} }",
            "mod nested { fn system() {} }",
            "make_systems!();",
            "const VALUE: bool = compute();",
        ] {
            assert!(!failures(source).is_empty(), "accepted {source}");
        }
    }

    #[test]
    fn contract_budget_counts_all_source_modules() {
        let root =
            std::env::temp_dir().join(format!("cinnabar-contract-budget-{}", std::process::id()));
        let source = root.join("hub/src");
        std::fs::create_dir_all(&source).unwrap();
        let files = [source.join("lib.rs"), source.join("observation.rs")];
        std::fs::write(&files[0], "mod observation;\n").unwrap();
        std::fs::write(&files[1], "pub struct Observation;\n").unwrap();
        let policy: Policy = toml::from_str(
            "production_rust_max = 1000
             module_root_max = 300
             powershell_max = 800
             test_max = 1200
             [[crates]]
             name = 'hub'
             path = 'hub'
             contract_only = true
             max_rust_lines = 1",
        )
        .unwrap();
        let mut diagnostics = Vec::new();
        check_contracts(&root, &policy, &files, &mut diagnostics).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            diagnostics,
            ["hub: 2 Rust source lines exceeds crate limit 1"]
        );
    }
}
