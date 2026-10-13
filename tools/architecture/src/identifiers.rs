use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use syn::visit::Visit;

use crate::{
    ArchitectureError,
    paths::{in_tree, relative_slash},
    policy::{IdentifierRules, Policy},
    read,
    rust_scope::{inherited_test_files, is_rust_test, reachable_production_files},
};

mod scan;
mod scopes;
mod targets;
#[cfg(test)]
mod tests;

use scan::RustScan;

/// Pinned full identifiers, bare spellings, and affixes shared by distinct names.
struct Vocabulary {
    full: BTreeSet<String>,
    bare: BTreeSet<String>,
    affixes: BTreeSet<String>,
}

impl Vocabulary {
    /// Derives bare names and proper affixes after deduplicating all vocabulary domains.
    fn new(full: BTreeSet<String>) -> Self {
        let bare: BTreeSet<String> = full
            .iter()
            .map(|name| {
                name.split_once(':')
                    .map_or(name.as_str(), |(_, bare)| bare)
                    .to_owned()
            })
            .collect();
        let mut counts = BTreeMap::<String, usize>::new();
        for name in &bare {
            let mut affixes = BTreeSet::new();
            for (index, _) in name.char_indices().skip(1) {
                affixes.insert(name[..index].to_owned());
                affixes.insert(name[index..].to_owned());
            }
            for affix in affixes {
                *counts.entry(affix).or_default() += 1;
            }
        }
        Self {
            full,
            bare,
            affixes: counts
                .into_iter()
                .filter_map(|(affix, count)| (count >= 2).then_some(affix))
                .collect(),
        }
    }

    /// Loads policy-selected text columns and decodes registries with the owning asset codec.
    fn load(
        root: &Path,
        rules: &IdentifierRules,
        diagnostics: &mut Vec<String>,
    ) -> Result<Self, ArchitectureError> {
        let mut full = BTreeSet::new();
        for source in &rules.vocabularies {
            for (line, row) in read(&root.join(&source.path))?.lines().enumerate() {
                if row.is_empty() || row.starts_with('#') {
                    continue;
                }
                match row
                    .split('\t')
                    .nth(source.column)
                    .filter(|name| !name.is_empty())
                {
                    Some(name) => {
                        full.insert(name.to_owned());
                    }
                    None => diagnostics.push(format!(
                        "{}:{}: missing identifier vocabulary column {}",
                        source.path,
                        line + 1,
                        source.column
                    )),
                }
            }
        }
        for relative in &rules.block_registries {
            let path = root.join(relative);
            let bytes =
                fs::read(&path).map_err(|source| ArchitectureError::Read { path, source })?;
            let records = assets::registry_header_protocol(&bytes)
                .and_then(|protocol| assets::read_registry_for_protocol(&bytes, protocol));
            match records {
                Ok(records) => full.extend(records.iter().map(|record| record.name.to_string())),
                Err(error) => diagnostics.push(format!(
                    "{relative}: cannot read identifier block registry: {error}"
                )),
            }
        }
        if full.is_empty() {
            diagnostics.push("identifier_literals: the pinned vocabulary is empty".into());
        }
        Ok(Self::new(full))
    }
}

/// Checks production literals against pinned vocabularies and per-file migration budgets.
pub(super) fn check_identifiers(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.identifier_literals;
    if rules.vocabularies.is_empty()
        && rules.block_registries.is_empty()
        && rules.owners.is_empty()
        && rules.baselines.is_empty()
        && policy.identifier_literal_exceptions.is_empty()
    {
        return Ok(());
    }
    let vocabulary = Vocabulary::load(root, rules, diagnostics)?;
    let features = policy
        .crate_rules
        .iter()
        .flat_map(|rule| rule.test_support_features.iter().cloned())
        .collect();
    let targets = targets::production_targets(root, policy, files, diagnostics)?;
    let mut scans = BTreeMap::new();
    for path in files {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if path.extension().is_none_or(|extension| extension != "rs")
            && !targets.production.contains(&canonical)
            && !targets.development.contains(&canonical)
        {
            continue;
        }
        let relative = relative_slash(root, path);
        match syn::parse_file(&read(path)?) {
            Ok(file) => {
                let mut scan = RustScan::new(
                    path,
                    targets.production.contains(&canonical)
                        || targets.development.contains(&canonical),
                    &features,
                    &vocabulary,
                );
                scan.visit_file(&file);
                scans.insert(canonical, (relative, scan));
            }
            Err(error) => diagnostics.push(format!(
                "{relative}:{}: cannot parse Rust for identifier_literals: {error}",
                error.span().start().line
            )),
        }
    }
    let test_files = inherited_test_files(scans.iter().map(|(path, (relative, scan))| {
        (
            path,
            scan.file_test
                || is_rust_test(relative)
                || targets::development_path(relative)
                || targets.development.contains(path),
            scan.modules.as_slice(),
        )
    }));
    let production_files = reachable_production_files(
        scans
            .iter()
            .map(|(path, (_, scan))| (path, scan.file_test, scan.modules.as_slice())),
        targets.production,
    );
    let mut used = vec![false; policy.identifier_literal_exceptions.len()];
    let mut exception_keys = BTreeSet::new();
    for exception in &policy.identifier_literal_exceptions {
        if exception.reason.trim().is_empty() {
            diagnostics.push(format!(
                "{}: identifier literal exception needs a reason",
                exception.path
            ));
        }
        if !exception_keys.insert((&exception.path, &exception.literal)) {
            diagnostics.push(format!(
                "{}: duplicate identifier literal exception for {:?}",
                exception.path, exception.literal
            ));
        }
    }
    let mut counts = BTreeMap::new();
    for (path, (relative, scan)) in scans {
        if scan.file_test
            || ((is_rust_test(&relative)
                || targets::development_path(&relative)
                || targets.development.contains(&path)
                || test_files.contains(&path))
                && !production_files.contains(&path))
            || rules.owners.iter().any(|owner| {
                if owner.ends_with('/') {
                    in_tree(&relative, owner)
                } else {
                    relative == owner.trim_start_matches("./")
                }
            })
        {
            continue;
        }
        let mut violations = Vec::new();
        for violation in scan
            .violations
            .into_values()
            .filter(|violation| !violation.test)
        {
            if let Some(index) = policy
                .identifier_literal_exceptions
                .iter()
                .position(|entry| entry.path == relative && entry.literal == violation.literal)
            {
                used[index] = true;
            } else {
                violations.push(violation);
            }
        }
        counts.insert(relative.clone(), violations.len());
        let budget = rules
            .baselines
            .iter()
            .find(|entry| entry.path == relative)
            .map_or(0, |entry| entry.max);
        if violations.len() > budget {
            diagnostics.push(format!("{relative}: identifier_literals count {} exceeds budget {budget}; observed baseline: {{ path = {relative:?}, max = {} }}", violations.len(), violations.len()));
            for violation in violations {
                diagnostics.push(format!("{relative}:{}: identifier_literals rejects {:?}; use the facts owner or a reasoned literal exception", violation.line, violation.literal));
            }
        }
    }
    for (exception, used) in policy.identifier_literal_exceptions.iter().zip(used) {
        if !used {
            diagnostics.push(format!(
                "{}: unused identifier literal exception for {:?}",
                exception.path, exception.literal
            ));
        }
    }
    check_budgets(rules, &counts, diagnostics);
    Ok(())
}

/// Rejects duplicate budgets; falling counts remain valid and can reduce the policy budget.
fn check_budgets(
    rules: &IdentifierRules,
    counts: &BTreeMap<String, usize>,
    diagnostics: &mut Vec<String>,
) {
    let mut paths = BTreeSet::new();
    for entry in &rules.baselines {
        if !paths.insert(&entry.path) {
            diagnostics.push(format!(
                "{}: duplicate identifier_literals budget",
                entry.path
            ));
        }
        if !counts.contains_key(&entry.path) {
            diagnostics.push(format!(
                "{}: unused identifier_literals budget; remove this entry",
                entry.path
            ));
        }
    }
}
