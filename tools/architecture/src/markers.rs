use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::{
    ArchitectureError,
    paths::{marker_literals, relative_slash},
    policy::{MarkerKind, Policy},
    read,
};

pub(super) fn check_markers(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let mut occurrences = BTreeMap::<String, Vec<String>>::new();
    for path in files {
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("rs" | "ps1")
        ) {
            continue;
        }
        let relative = relative_slash(root, path);
        if !relative.starts_with("app/src/")
            && relative != "scripts/acceptance.ps1"
            && !relative.starts_with("scripts/acceptance/")
        {
            continue;
        }
        for marker in marker_literals(&read(path)?) {
            occurrences
                .entry(marker)
                .or_default()
                .push(relative.clone());
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
            .filter(|path| *path == &rule.producer)
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
                .map(|consumer| entries.iter().filter(|path| *path == consumer).count())
                .unwrap_or(0);
            if consumer_count == 0 {
                diagnostics.push(format!(
                    "marker `{}` has no occurrence in declared consumer {}",
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
