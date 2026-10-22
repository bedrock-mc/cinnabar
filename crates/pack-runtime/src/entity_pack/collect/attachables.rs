//! Changed binding dependencies load installed definitions; texture-only changes keep startup bindings.

use super::*;

/// Collects authored bindings and installed definitions whose binding dependencies change.
pub(super) fn collect(
    view: &LayeredPackView,
    vanilla_pack_dir: Option<&Path>,
    overrides: &BTreeSet<String>,
    vanilla: Option<&VanillaEntityRefs>,
    inputs: Option<&pack_entity_compiler::EquipmentBindingDependencies>,
) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files = view
        .list("attachables/")
        .into_iter()
        .filter(|path| path.ends_with(".json"))
        .filter_map(|path| Some((Box::<str>::from(path), canonical_json(&view.read(path)?)?)))
        .collect::<Vec<_>>();
    let Some(root) = vanilla_pack_dir.filter(|_| !overrides.is_empty()) else {
        return files;
    };
    let authored_paths = files
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    let authored_items = files
        .iter()
        .filter_map(|(_, bytes)| parse_pack_json(bytes))
        .flat_map(|value| bound_items(&value))
        .collect::<BTreeSet<_>>();
    let mut installed = Vec::new();
    let mut directories = vec![root.join("attachables")];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                directories.push(entry.path());
                continue;
            }
            let path = entry.path();
            if !kind.is_file() || path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            let mut bytes = Vec::new();
            if file
                .take(resource_pack::MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || bytes.len() as u64 > resource_pack::MAX_FILE_BYTES
            {
                continue;
            }
            let Some(value) = parse_pack_json(&bytes) else {
                continue;
            };
            installed.push((relative, value));
        }
    }
    let authored = files
        .iter()
        .filter_map(|(_, bytes)| parse_pack_json(bytes))
        .collect::<Vec<_>>();
    if let Ok(missing) = pack_entity_compiler::missing_harness_bindings(
        |path| root.join(path).is_file(),
        installed
            .iter()
            .map(|(_, value)| value)
            .chain(authored.iter()),
    ) {
        installed.extend(
            missing
                .into_iter()
                .map(|(path, value)| (path.into_string(), value)),
        );
    }
    let mut candidates = Vec::new();
    let mut changed_items = BTreeSet::new();
    for (path, value) in installed {
        let items = bound_items(&value);
        if authored_paths.contains(path.as_str())
            || items.iter().any(|item| authored_items.contains(item))
        {
            continue;
        }
        if !binding_inputs_unchanged(&value, &authored_items, overrides, vanilla, inputs) {
            changed_items.extend(items.iter().cloned());
        }
        candidates.push((path, items, value));
    }
    // Each item's variants share the decision, preserving the compiler's player preference.
    for (path, items, value) in candidates {
        if items.iter().any(|item| changed_items.contains(item))
            && let Ok(bytes) = serde_json::to_vec(&value)
        {
            files.push((path.into(), bytes));
        }
    }
    // File-system enumeration order must not decide which variant wins compilation.
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Identifies every item an attachable binds, including owner-specific variants.
fn bound_items(value: &Value) -> Vec<String> {
    let description = &value["minecraft:attachable"]["description"];
    if let Some(items) = description["item"].as_object() {
        return items.keys().cloned().collect();
    }
    description["identifier"]
        .as_str()
        .map(str::to_owned)
        .into_iter()
        .collect()
}

/// Checks whether a variant's binding dependencies remain unchanged by the stack.
pub(super) fn binding_inputs_unchanged(
    document: &Value,
    authored_items: &BTreeSet<String>,
    overrides: &BTreeSet<String>,
    vanilla: Option<&VanillaEntityRefs>,
    inputs: Option<&pack_entity_compiler::EquipmentBindingDependencies>,
) -> bool {
    let Some(mut references) = inputs.and_then(|inputs| inputs.names(document).ok()) else {
        // Unreadable inputs cannot prove the installed binding remains unchanged.
        return false;
    };
    let mut pending = references.iter().cloned().collect::<Vec<_>>();
    while let Some(geometry) = pending.pop() {
        if let Some(parent) = vanilla.and_then(|refs| refs.geometry_parent.get(&geometry))
            && references.insert(parent.clone())
        {
            pending.push(parent.clone());
        }
    }
    bound_items(document)
        .iter()
        .all(|item| !authored_items.contains(item))
        && references.is_disjoint(overrides)
}
