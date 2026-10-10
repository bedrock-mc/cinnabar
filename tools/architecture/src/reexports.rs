use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use syn::{Item, UseTree, Visibility};

use crate::{
    ArchitectureError,
    conditions::{Condition, Configuration, any_enabled},
    paths::relative_slash,
    policy::Policy,
    read,
};

type Name = Vec<String>;
type ModuleFile = (PathBuf, Name, Condition);

struct Import {
    condition: Condition,
    module: Name,
    target: Name,
}

struct Export {
    condition: Condition,
    file: String,
    module: Name,
    target: Name,
    name: String,
}

#[derive(Default)]
struct Symbols {
    dependencies: BTreeSet<String>,
    modules: BTreeMap<Name, Vec<Condition>>,
    definitions: BTreeMap<Name, Vec<Condition>>,
    globs: BTreeMap<Name, Vec<Import>>,
    imports: BTreeMap<Name, Vec<Import>>,
    exports: Vec<Export>,
}

/// Rejects forwarding APIs and visible glob imports, including restricted visibility.
pub(super) fn check_reexports(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let root = normalized_path(root);
    let files = files
        .iter()
        .map(|path| normalized_path(path))
        .collect::<Vec<_>>();
    for rule in &policy.crate_rules {
        let directory = normalized_path(&root.join(&rule.path));
        let manifest_path = directory.join("Cargo.toml");
        let manifest = toml::from_str::<toml::Value>(&read(&manifest_path)?).map_err(|source| {
            ArchitectureError::Policy {
                path: manifest_path,
                source,
            }
        })?;
        let mut dependencies = BTreeSet::new();
        dependency_names(&manifest, &mut dependencies);
        dependencies.extend(["std".into(), "core".into(), "alloc".into()]);
        let mut parsed_files = BTreeMap::new();
        for path in files.iter().filter(|path| {
            path.starts_with(&directory) && path.extension().is_some_and(|ext| ext == "rs")
        }) {
            let source = read(path)?;
            // Rust compilation diagnoses invalid source; fixtures may deliberately contain it.
            let Ok(parsed) = syn::parse_file(&source) else {
                continue;
            };
            parsed_files.insert(path.clone(), parsed);
        }
        for tree in module_trees(&directory, &manifest, &parsed_files) {
            let mut symbols = Symbols {
                dependencies: dependencies.clone(),
                ..Symbols::default()
            };
            for (path, module, condition) in tree {
                let condition = condition.with_attrs(&parsed_files[&path].attrs);
                if module.len() > 1 {
                    symbols
                        .modules
                        .entry(module.clone())
                        .or_default()
                        .push(condition.clone());
                }
                collect(
                    &parsed_files[&path].items,
                    &module,
                    &relative_slash(&root, &path),
                    &condition,
                    &mut symbols,
                    diagnostics,
                );
            }
            for export in &symbols.exports {
                let allowed = policy.reexport_allowances.iter().any(|allowance| {
                    allowance.path == export.file && allowance.exports.contains(&export.name)
                });
                if !allowed && let Some(target) = external_target(export, &symbols) {
                    diagnostics.push(format!(
                        "{}: cross-crate re-export `{}` is forbidden; import from `{}` directly",
                        export.file,
                        export.name,
                        target.join("::")
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Keeps separate target roots isolated while following ordinary and explicit module paths.
fn module_trees(
    directory: &Path,
    manifest: &toml::Value,
    files: &BTreeMap<PathBuf, syn::File>,
) -> Vec<Vec<ModuleFile>> {
    let targets = target_roots(directory, manifest, files);
    let mut edges = BTreeMap::new();
    let mut children = BTreeSet::new();
    for (path, file) in files {
        let parent = path.parent().unwrap_or(directory);
        let base = if targets.contains(path)
            || matches!(
                path.file_stem().and_then(|name| name.to_str()),
                Some("lib" | "main" | "mod")
            ) {
            parent.to_path_buf()
        } else {
            path.with_extension("")
        };
        let mut declared = Vec::new();
        module_edges(
            &file.items,
            &base,
            parent,
            &[],
            &Condition::default(),
            &mut declared,
        );
        declared.retain(|(path, _, _)| files.contains_key(path));
        children.extend(declared.iter().map(|(path, _, _)| path.clone()));
        edges.insert(path.clone(), declared);
    }
    let mut trees = Vec::new();
    for root in files
        .keys()
        .filter(|path| targets.contains(*path) || !children.contains(*path))
    {
        let mut pending = vec![(
            root.clone(),
            vec!["crate".into()],
            Condition::default(),
            BTreeSet::new(),
        )];
        let mut tree = Vec::new();
        while let Some((path, module, condition, mut ancestors)) = pending.pop() {
            if !ancestors.insert(path.clone()) {
                continue;
            }
            for (child, suffix, child_condition) in &edges[&path] {
                let mut name = module.clone();
                name.extend(suffix.iter().cloned());
                pending.push((
                    child.clone(),
                    name,
                    Condition::All(vec![
                        condition.clone(),
                        Condition::default().with_attrs(&files[&path].attrs),
                        child_condition.clone(),
                    ]),
                    ancestors.clone(),
                ));
            }
            tree.push((path, module, condition));
        }
        trees.push(tree);
    }
    trees
}

/// Identifies conventional and explicitly configured Cargo entry files, even when also imported.
fn target_roots(
    directory: &Path,
    manifest: &toml::Value,
    files: &BTreeMap<PathBuf, syn::File>,
) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::from([directory.join("src/lib.rs"), directory.join("src/main.rs")]);
    for kind in ["lib", "bin", "test", "bench", "example"] {
        let Some(value) = manifest.get(kind) else {
            continue;
        };
        let targets = value
            .as_array()
            .map_or_else(|| vec![value], |values| values.iter().collect());
        for target in targets {
            if let Some(path) = target.get("path").and_then(toml::Value::as_str) {
                roots.insert(normalized_path(&directory.join(path)));
            }
        }
    }
    for path in files.keys() {
        for folder in ["src/bin", "tests", "benches", "examples"] {
            let Ok(relative) = path.strip_prefix(directory.join(folder)) else {
                continue;
            };
            let parts = relative.components().count();
            if parts == 1
                || (parts == 2 && relative.file_name().is_some_and(|name| name == "main.rs"))
            {
                roots.insert(path.clone());
            }
        }
    }
    roots
}

/// Resolves ordinary and explicit external module files beneath inline modules.
fn module_edges(
    items: &[Item],
    directory: &Path,
    explicit_base: &Path,
    prefix: &[String],
    condition: &Condition,
    output: &mut Vec<ModuleFile>,
) {
    for item in items {
        let Item::Mod(item) = item else { continue };
        let condition = condition.with_attrs(&item.attrs);
        let mut name = prefix.to_vec();
        name.push(item.ident.to_string());
        let nested = directory.join(item.ident.to_string());
        if let Some((_, items)) = &item.content {
            module_edges(items, &nested, &nested, &name, &condition, output);
            continue;
        }
        let explicit = item.attrs.iter().find_map(|attribute| {
            if !attribute.path().is_ident("path") {
                return None;
            }
            let syn::Meta::NameValue(value) = &attribute.meta else {
                return None;
            };
            let syn::Expr::Lit(value) = &value.value else {
                return None;
            };
            let syn::Lit::Str(value) = &value.lit else {
                return None;
            };
            Some(explicit_base.join(value.value()))
        });
        let candidates = explicit.map_or_else(
            || vec![nested.with_extension("rs"), nested.join("mod.rs")],
            |path| vec![path],
        );
        for path in candidates {
            output.push((normalized_path(&path), name.clone(), condition.clone()));
        }
    }
}

/// Uses one lexical path form for roots, parsed files and module edges without dropping leading parents.
fn normalized_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

/// Collects dependency keys, including renamed and target-specific dependencies.
fn dependency_names(value: &toml::Value, names: &mut BTreeSet<String>) {
    let Some(table) = value.as_table() else {
        return;
    };
    for (key, value) in table {
        if matches!(
            key.as_str(),
            "dependencies" | "dev-dependencies" | "build-dependencies"
        ) {
            if let Some(dependencies) = value.as_table() {
                names.extend(dependencies.keys().map(|name| name.replace('-', "_")));
            }
        } else if key == "target" {
            if let Some(targets) = value.as_table() {
                for target in targets.values() {
                    dependency_names(target, names);
                }
            }
        }
    }
}

/// Records imports before resolving them so declaration order does not affect enforcement.
fn collect(
    items: &[Item],
    module: &[String],
    file: &str,
    condition: &Condition,
    symbols: &mut Symbols,
    diagnostics: &mut Vec<String>,
) {
    for item in items {
        let condition = condition.with_item(item);
        match item {
            Item::Mod(item) => {
                let mut child = module.to_vec();
                child.push(item.ident.to_string());
                if let Some((_, items)) = &item.content {
                    symbols
                        .modules
                        .entry(child.clone())
                        .or_default()
                        .push(condition.clone());
                    collect(items, &child, file, &condition, symbols, diagnostics);
                }
            }
            Item::Use(item) => {
                let mut leaves = Vec::new();
                use_paths(&item.tree, &mut Vec::new(), &mut leaves);
                for (mut target, binding) in leaves {
                    if item.leading_colon.is_some() {
                        target.insert(0, String::new());
                    }
                    if binding == "*" {
                        symbols
                            .globs
                            .entry(module.to_vec())
                            .or_default()
                            .push(Import {
                                module: module.to_vec(),
                                target: target.clone(),
                                condition: condition.clone(),
                            });
                        if !matches!(item.vis, Visibility::Inherited) {
                            diagnostics.push(format!(
                                "{file}: glob re-export `{}` is forbidden",
                                target.join("::")
                            ));
                        }
                        continue;
                    }
                    let mut key = module.to_vec();
                    key.push(binding.clone());
                    symbols.imports.entry(key).or_default().push(Import {
                        condition: condition.clone(),
                        module: module.to_vec(),
                        target: target.clone(),
                    });
                    if !matches!(item.vis, Visibility::Inherited) {
                        let mut name = target.join("::");
                        if target.last() != Some(&binding) {
                            name.push_str(&format!(" as {binding}"));
                        }
                        symbols.exports.push(Export {
                            condition: condition.clone(),
                            file: file.into(),
                            module: module.to_vec(),
                            target,
                            name,
                        });
                    }
                }
            }
            Item::ExternCrate(item) => {
                let name = item.ident.to_string();
                let binding = item
                    .rename
                    .as_ref()
                    .map_or_else(|| name.clone(), |(_, name)| name.to_string());
                symbols.dependencies.insert(name.clone());
                let mut key = module.to_vec();
                key.push(binding.clone());
                symbols.imports.entry(key).or_default().push(Import {
                    condition: condition.clone(),
                    module: module.to_vec(),
                    target: vec![name.clone()],
                });
                if !matches!(item.vis, Visibility::Inherited) {
                    diagnostics.push(format!("{file}: cross-crate re-export `extern crate {name} as {binding}` is forbidden"));
                }
            }
            _ => {
                let name = match item {
                    Item::Struct(item) => Some(&item.ident),
                    Item::Enum(item) => Some(&item.ident),
                    Item::Union(item) => Some(&item.ident),
                    Item::Type(item) => Some(&item.ident),
                    Item::Trait(item) => Some(&item.ident),
                    Item::TraitAlias(item) => Some(&item.ident),
                    Item::Fn(item) => Some(&item.sig.ident),
                    Item::Const(item) => Some(&item.ident),
                    Item::Static(item) => Some(&item.ident),
                    _ => None,
                };
                if let Some(name) = name {
                    let mut key = module.to_vec();
                    key.push(name.to_string());
                    symbols
                        .definitions
                        .entry(key)
                        .or_default()
                        .push(condition.clone());
                }
            }
        }
    }
}

/// Flattens grouped, renamed and self imports without relying on source formatting.
fn use_paths(tree: &UseTree, prefix: &mut Name, output: &mut Vec<(Name, String)>) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_paths(&path.tree, prefix, output);
            prefix.pop();
        }
        UseTree::Group(group) => {
            for child in &group.items {
                use_paths(child, prefix, output);
            }
        }
        UseTree::Name(name) => {
            let mut path = prefix.clone();
            if name.ident != "self" {
                path.push(name.ident.to_string());
            }
            if let Some(binding) = path.last().cloned() {
                output.push((path, binding));
            }
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            if rename.ident != "self" {
                path.push(rename.ident.to_string());
            }
            output.push((path, rename.rename.to_string()));
        }
        UseTree::Glob(_) => output.push((prefix.clone(), "*".into())),
    }
}

/// Resolves local forwarding chains while allowing a module to shadow a dependency name.
fn resolve(
    module: &[String],
    path: &[String],
    symbols: &Symbols,
    seen: &mut BTreeSet<Name>,
    configuration: &Configuration,
) -> Result<Name, String> {
    let mut absolute = module.to_vec();
    let mut index = 0;
    if path.first().is_some_and(String::is_empty) {
        absolute.clear();
        index = 1;
    } else if path.first().is_some_and(|name| name == "crate") {
        absolute.clear();
    } else if path
        .first()
        .is_some_and(|name| name == "self" || name == "super")
    {
        while let Some(name) = path.get(index) {
            match name.as_str() {
                "self" => {}
                "super" => {
                    absolute.pop();
                }
                _ => break,
            }
            index += 1;
        }
    } else if let Some(first) = path.first() {
        let mut local = module.to_vec();
        local.push(first.clone());
        let mut has_import = false;
        let mut imports_self = false;
        if let Some(imports) = symbols.imports.get(&local) {
            for import in imports {
                if import.condition.enabled(configuration)? {
                    has_import = true;
                    imports_self |= import.module == module
                        && import.target.len() == 1
                        && import.target[0] == *first;
                }
            }
        }
        if symbols.dependencies.contains(first)
            && !active_name(&symbols.modules, &local, configuration)?
            && !active_name(&symbols.definitions, &local, configuration)?
            && (!has_import || imports_self)
        {
            absolute.clear();
        }
    }
    absolute.extend_from_slice(&path[index..]);
    for length in (2..=absolute.len()).rev() {
        let prefix = absolute[..length].to_vec();
        if let Some(imports) = symbols.imports.get(&prefix) {
            if seen.insert(prefix) {
                let mut local = None;
                for import in imports {
                    if !import.condition.enabled(configuration)? {
                        continue;
                    }
                    let mut branch_seen = seen.clone();
                    let mut target = resolve(
                        &import.module,
                        &import.target,
                        symbols,
                        &mut branch_seen,
                        configuration,
                    )?;
                    target.extend_from_slice(&absolute[length..]);
                    if length < absolute.len() && target.first().is_some_and(|name| name == "crate")
                    {
                        target =
                            resolve(module, &target, symbols, &mut branch_seen, configuration)?;
                    }
                    if target.first().is_some_and(|name| name != "crate") {
                        return Ok(target);
                    }
                    local.get_or_insert(target);
                }
                if let Some(local) = local {
                    return Ok(local);
                }
            }
        }
    }
    resolve_globs(absolute, symbols, seen, configuration)
}

/// Resolves names supplied by private globs while preserving explicit local definitions.
fn resolve_globs(
    absolute: Name,
    symbols: &Symbols,
    seen: &mut BTreeSet<Name>,
    configuration: &Configuration,
) -> Result<Name, String> {
    if absolute.first().is_none_or(|name| name != "crate") {
        return Ok(absolute);
    }
    for index in 1..absolute.len() {
        let binding = &absolute[..=index];
        if active_name(&symbols.definitions, binding, configuration)? {
            return Ok(absolute);
        }
        if active_name(&symbols.modules, binding, configuration)? {
            continue;
        }
        let module = &absolute[..index];
        if let Some(globs) = symbols.globs.get(module) {
            let mut external = None;
            for glob in globs {
                if !glob.condition.enabled(configuration)? {
                    continue;
                }
                let mut target = glob.target.clone();
                target.extend_from_slice(&absolute[index..]);
                let mut key = module.to_vec();
                key.push("*".into());
                key.extend(target.iter().cloned());
                let mut branch_seen = seen.clone();
                if !branch_seen.insert(key) {
                    continue;
                }
                let resolved = resolve(module, &target, symbols, &mut branch_seen, configuration)?;
                if resolved.first().is_some_and(|name| name != "crate") {
                    external.get_or_insert(resolved);
                } else if known_local(&resolved, symbols, configuration)? {
                    return Ok(resolved);
                }
            }
            if let Some(external) = external {
                return Ok(external);
            }
        }
        break;
    }
    Ok(absolute)
}

/// Finds a forwarding configuration while expanding only predicates used by this export.
fn external_target(export: &Export, symbols: &Symbols) -> Option<Name> {
    let mut pending = vec![Configuration::new()];
    while let Some(configuration) = pending.pop() {
        let result = export
            .condition
            .enabled(&configuration)
            .and_then(|enabled| {
                if enabled {
                    resolve(
                        &export.module,
                        &export.target,
                        symbols,
                        &mut BTreeSet::new(),
                        &configuration,
                    )
                    .map(Some)
                } else {
                    Ok(None)
                }
            });
        match result {
            Ok(Some(target)) if target.first().is_some_and(|name| name != "crate") => {
                return Some(target);
            }
            Ok(_) => {}
            Err(atom) => {
                let mut disabled = configuration.clone();
                disabled.insert(atom.clone(), false);
                pending.push(disabled);
                let mut enabled = configuration;
                enabled.insert(atom, true);
                pending.push(enabled);
            }
        }
    }
    None
}

/// Checks whether a locally declared module or definition exists in this configuration.
fn active_name(
    names: &BTreeMap<Name, Vec<Condition>>,
    name: &[String],
    configuration: &Configuration,
) -> Result<bool, String> {
    names.get(name).map_or(Ok(false), |conditions| {
        any_enabled(conditions, configuration)
    })
}

/// Recognizes active local modules, types and their associated members.
fn known_local(
    name: &[String],
    symbols: &Symbols,
    configuration: &Configuration,
) -> Result<bool, String> {
    if active_name(&symbols.modules, name, configuration)? {
        return Ok(true);
    }
    for length in 1..=name.len() {
        if active_name(&symbols.definitions, &name[..length], configuration)? {
            return Ok(true);
        }
    }
    Ok(false)
}
