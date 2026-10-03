use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

use syn::{Attribute, Item, Meta, Token, Type, UseTree, punctuated::Punctuated, visit::Visit};

use crate::{
    ArchitectureError,
    paths::relative_slash,
    policy::{ModuleBoundary, Policy},
    read,
};

/// Checks configured production module seams without treating comments or test code as edges.
pub(super) fn check_modules(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let mut parsed_files = BTreeMap::new();
    for path in files {
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let relative = relative_slash(root, path);
        if !policy
            .module_boundaries
            .iter()
            .any(|rule| matches_rule(&relative, rule))
        {
            continue;
        }
        match syn::parse_file(&read(path)?) {
            Ok(parsed) => {
                parsed_files.insert(path.clone(), parsed);
            }
            Err(error) => {
                diagnostics.push(format!("{relative}: cannot parse module boundary: {error}"))
            }
        }
    }
    let production_files = production_files(&parsed_files);
    let symbols = collect_symbols(&parsed_files, &production_files);
    for (path, parsed) in &parsed_files {
        if !production_files.contains(path) {
            continue;
        }
        let relative = relative_slash(root, path);
        let rules = policy
            .module_boundaries
            .iter()
            .filter(|rule| matches_rule(&relative, rule));
        for rule in rules {
            let mut visitor = ModuleVisitor {
                rule,
                relative: &relative,
                aliases: BTreeMap::new(),
                type_aliases: BTreeMap::new(),
                module: file_module(path),
                symbols: &symbols,
                diagnostics,
            };
            visitor.visit_file(parsed);
        }
    }
    Ok(())
}

/// Matches the root file and descendants of a configured module boundary.
fn matches_rule(relative: &str, rule: &ModuleBoundary) -> bool {
    relative == format!("{}.rs", rule.path)
        || relative == rule.path
        || relative.starts_with(&format!("{}/", rule.path))
}

/// Follows external module declarations so parent cfg(test) applies to their files too.
fn production_files(parsed: &BTreeMap<PathBuf, syn::File>) -> BTreeSet<PathBuf> {
    let mut edges = BTreeMap::new();
    let mut children = BTreeSet::new();
    for (path, file) in parsed {
        let parent = path.parent().unwrap_or(Path::new(""));
        let directory = if matches!(
            path.file_name().and_then(|name| name.to_str()),
            Some("mod.rs" | "lib.rs" | "main.rs")
        ) {
            parent.to_path_buf()
        } else {
            path.with_extension("")
        };
        let mut declared = Vec::new();
        module_files(&file.items, &directory, parent, true, &mut declared);
        declared.retain(|(path, _)| parsed.contains_key(path));
        children.extend(declared.iter().map(|(path, _)| path.clone()));
        edges.insert(path.clone(), declared);
    }
    let mut pending: Vec<_> = parsed
        .keys()
        .filter(|path| !children.contains(*path))
        .cloned()
        .collect();
    let mut reachable = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if (!children.contains(&path) && test_path(&path))
            || !production(&parsed[&path].attrs)
            || !reachable.insert(path.clone())
        {
            continue;
        }
        pending.extend(
            edges[&path]
                .iter()
                .filter(|(_, active)| *active)
                .map(|(path, _)| path.clone()),
        );
    }
    reachable
}

/// Resolves ordinary and explicit module paths while retaining each declaration's cfg state.
fn module_files(
    items: &[Item],
    directory: &Path,
    explicit_base: &Path,
    active: bool,
    output: &mut Vec<(PathBuf, bool)>,
) {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        let active = active && production(&module.attrs);
        let nested = directory.join(module.ident.to_string());
        if let Some((_, items)) = &module.content {
            module_files(items, &nested, &nested, active, output);
            continue;
        }
        let explicit = module.attrs.iter().find_map(|attribute| {
            if !attribute.path().is_ident("path") {
                return None;
            }
            let Meta::NameValue(value) = &attribute.meta else {
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
        if let Some(path) = explicit {
            output.push((path, active));
        } else {
            output.push((nested.with_extension("rs"), active));
            output.push((nested.join("mod.rs"), active));
        }
    }
}

/// Recognizes separate test files using the repository's existing source categories.
fn test_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || path
            .components()
            .any(|component| matches!(component, Component::Normal(name) if name == "tests"))
}

/// Determines whether a cfg condition can be active in any non-test build.
fn production_cfg(meta: &Meta) -> Option<bool> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Some(false),
        Meta::List(list) => {
            let children = list
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .ok()?;
            let values: Vec<_> = children.iter().map(production_cfg).collect();
            if list.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|value| *value == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if list.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|value| *value == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else if list.path.is_ident("not") && values.len() == 1 {
                values[0].map(|value| !value)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Skips only items proved inactive outside tests; platform and feature branches remain checked.
fn production(attrs: &[Attribute]) -> bool {
    attrs.iter().all(|attribute| {
        !attribute.path().is_ident("cfg")
            || attribute
                .parse_args::<Meta>()
                .ok()
                .and_then(|meta| production_cfg(&meta))
                != Some(false)
    })
}

/// Returns item attributes so cfg(test) also excludes structs, imports and functions.
fn attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// Expands grouped and renamed imports into complete paths and local names.
fn use_paths(tree: &UseTree, prefix: &mut Vec<String>, output: &mut Vec<(Vec<String>, String)>) {
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
            path.push(name.ident.to_string());
            output.push((path, name.ident.to_string()));
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.clone();
            path.push(rename.ident.to_string());
            output.push((path, rename.rename.to_string()));
        }
        UseTree::Glob(_) => output.push((prefix.clone(), String::new())),
    }
}

struct Symbol {
    module: Vec<String>,
    ty: Type,
}

/// Names a source module from its crate's src directory.
fn file_module(path: &Path) -> Vec<String> {
    let parts: Vec<_> = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let start = parts
        .iter()
        .rposition(|part| part == "src")
        .map_or(0, |index| index + 1);
    let mut module = vec!["crate".to_owned()];
    module.extend(parts[start..parts.len() - 1].iter().cloned());
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    if !matches!(stem, "lib" | "main" | "mod") {
        module.push(stem.to_owned());
    }
    module
}

/// Resolves a module-relative path without guessing external crate definitions.
fn symbol_path(module: &[String], path: &[String]) -> Vec<String> {
    let mut result = module.to_vec();
    let mut index = 0;
    if path.first().is_some_and(|part| part == "crate") {
        return path.to_vec();
    }
    while let Some(part) = path.get(index) {
        match part.as_str() {
            "self" => {}
            "super" => {
                result.pop();
            }
            _ => break,
        }
        index += 1;
    }
    result.extend_from_slice(&path[index..]);
    result
}

/// Collects production aliases across files so ownership cannot hide in a sibling module.
fn collect_symbols(
    parsed: &BTreeMap<PathBuf, syn::File>,
    production: &BTreeSet<PathBuf>,
) -> BTreeMap<Vec<String>, Symbol> {
    let mut symbols = BTreeMap::new();
    for path in production {
        collect_item_symbols(&parsed[path].items, &file_module(path), &mut symbols);
    }
    symbols
}

/// Records type aliases and renamed imports under their declaring module's path.
fn collect_item_symbols(
    items: &[Item],
    module: &[String],
    symbols: &mut BTreeMap<Vec<String>, Symbol>,
) {
    for item in items {
        if !production(attributes(item)) {
            continue;
        }
        match item {
            Item::Type(alias) => {
                let mut key = module.to_vec();
                key.push(alias.ident.to_string());
                symbols.insert(
                    key,
                    Symbol {
                        module: module.to_vec(),
                        ty: (*alias.ty).clone(),
                    },
                );
            }
            Item::Use(import) => {
                let mut paths = Vec::new();
                use_paths(&import.tree, &mut Vec::new(), &mut paths);
                for (path, local) in paths {
                    if local.is_empty() {
                        continue;
                    }
                    let mut key = module.to_vec();
                    key.push(local);
                    if let Ok(ty) = syn::parse_str::<Type>(&path.join("::")) {
                        symbols.insert(
                            key,
                            Symbol {
                                module: module.to_vec(),
                                ty,
                            },
                        );
                    }
                }
            }
            Item::Mod(nested) => {
                if let Some((_, items)) = &nested.content {
                    let mut path = module.to_vec();
                    path.push(nested.ident.to_string());
                    collect_item_symbols(items, &path, symbols);
                }
            }
            _ => {}
        }
    }
}

struct ModuleVisitor<'a> {
    rule: &'a ModuleBoundary,
    relative: &'a str,
    aliases: BTreeMap<String, String>,
    type_aliases: BTreeMap<String, Type>,
    module: Vec<String>,
    symbols: &'a BTreeMap<Vec<String>, Symbol>,
    diagnostics: &'a mut Vec<String>,
}

impl ModuleVisitor<'_> {
    /// Collects aliases before visiting a lexical scope, including forward type aliases.
    fn scope<'a>(&mut self, items: impl IntoIterator<Item = &'a Item>) {
        for item in items {
            if !production(attributes(item)) {
                continue;
            }
            match item {
                Item::Use(item) => {
                    let mut paths = Vec::new();
                    use_paths(&item.tree, &mut Vec::new(), &mut paths);
                    for (path, local) in paths {
                        if let Some(original) = path.last().filter(|_| !local.is_empty()) {
                            let key = symbol_path(&self.module, &path);
                            if self.symbols.contains_key(&key) {
                                let ty = syn::parse_str::<Type>(&key.join("::"))
                                    .expect("module symbol path");
                                self.type_aliases.insert(local.clone(), ty);
                            }
                            self.aliases.insert(local, original.clone());
                        }
                    }
                }
                Item::Type(item) => {
                    self.type_aliases
                        .insert(item.ident.to_string(), (*item.ty).clone());
                }
                _ => {}
            }
        }
    }

    /// Rejects a forbidden module appearing in a qualified path or grouped import.
    fn check_path(&mut self, path: &[String]) {
        for segment in path {
            let name = resolved_name(segment, &self.aliases);
            if self.rule.forbidden_modules.contains(&name) {
                self.diagnostics.push(format!(
                    "{}: production module path `{}` crosses forbidden `{name}` boundary",
                    self.relative,
                    path.join("::"),
                ));
            }
        }
    }

    /// Rejects owned authority in struct, enum or union fields, while allowing borrowed views.
    fn check_fields<'a>(&mut self, owner: &str, fields: impl IntoIterator<Item = &'a syn::Field>) {
        for field in fields {
            if !production(&field.attrs) {
                continue;
            }
            let mut owned = OwnedTypes {
                aliases: &self.aliases,
                type_aliases: &self.type_aliases,
                forbidden: &self.rule.forbidden_owned_types,
                expanding: BTreeSet::new(),
                module: self.module.clone(),
                symbols: self.symbols,
                found: BTreeSet::new(),
            };
            owned.visit_type(&field.ty);
            for name in owned.found {
                if self.rule.ownership_exceptions.iter().any(|exception| {
                    exception.path == self.relative
                        && exception.owner == owner
                        && exception.owned_type == name
                }) {
                    continue;
                }
                let field = field
                    .ident
                    .as_ref()
                    .map_or_else(|| "<tuple>".to_owned(), ToString::to_string);
                self.diagnostics.push(format!(
                    "{}: `{owner}.{field}` owns forbidden authority `{name}`",
                    self.relative,
                ));
            }
        }
    }
}

impl<'ast> Visit<'ast> for ModuleVisitor<'_> {
    /// Seeds aliases from the file's production items before checking it.
    fn visit_file(&mut self, file: &'ast syn::File) {
        if !production(&file.attrs) {
            return;
        }
        self.scope(&file.items);
        syn::visit::visit_file(self, file);
    }

    /// Ignores test-only items before any of their paths or fields can be inspected.
    fn visit_item(&mut self, item: &'ast Item) {
        if production(attributes(item)) {
            syn::visit::visit_item(self, item);
        }
    }

    /// Checks inline modules in their own alias scope.
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if let Some((_, items)) = &item.content {
            let saved = (self.aliases.clone(), self.type_aliases.clone());
            self.module.push(item.ident.to_string());
            self.scope(items);
            for item in items {
                self.visit_item(item);
            }
            (self.aliases, self.type_aliases) = saved;
            self.module.pop();
        }
    }

    /// Checks imports and re-exports even when a grouped import contains no syn::Path.
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut paths = Vec::new();
        use_paths(&item.tree, &mut Vec::new(), &mut paths);
        for (path, _) in paths {
            self.check_path(&path);
        }
    }

    /// Checks expression and type paths, including super paths and renamed imports.
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_path(
            &path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        );
        syn::visit::visit_path(self, path);
    }

    /// Checks retained struct fields before visiting their ordinary path dependencies.
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.check_fields(&item.ident.to_string(), &item.fields);
        syn::visit::visit_item_struct(self, item);
    }

    /// Includes enum payloads so an authority cannot hide behind a variant.
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        for variant in &item.variants {
            if production(&variant.attrs) {
                self.check_fields(&item.ident.to_string(), &variant.fields);
            }
        }
        syn::visit::visit_item_enum(self, item);
    }

    /// Includes union fields in the same ownership rule.
    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        self.check_fields(&item.ident.to_string(), &item.fields.named);
        syn::visit::visit_item_union(self, item);
    }

    /// Keeps cfg(test) fields out of both ownership and ordinary path checks.
    fn visit_field(&mut self, field: &'ast syn::Field) {
        if production(&field.attrs) {
            syn::visit::visit_field(self, field);
        }
    }

    /// Keeps cfg(test) variants out of ordinary path checks too.
    fn visit_variant(&mut self, variant: &'ast syn::Variant) {
        if production(&variant.attrs) {
            syn::visit::visit_variant(self, variant);
        }
    }

    /// Includes local aliases in their lexical block without leaking them outward.
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let saved = (self.aliases.clone(), self.type_aliases.clone());
        self.scope(block.stmts.iter().filter_map(|stmt| match stmt {
            syn::Stmt::Item(item) => Some(item),
            _ => None,
        }));
        syn::visit::visit_block(self, block);
        (self.aliases, self.type_aliases) = saved;
    }

    /// Excludes test-only methods without hiding production methods in the same impl.
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if production(&item.attrs) {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
}

/// Resolves local import aliases with a cycle guard.
fn resolved_name(name: &str, aliases: &BTreeMap<String, String>) -> String {
    let mut name = name.to_owned();
    let mut visited = BTreeSet::new();
    while visited.insert(name.clone()) {
        let Some(next) = aliases.get(&name) else {
            break;
        };
        name = next.clone();
    }
    name
}

struct OwnedTypes<'a> {
    aliases: &'a BTreeMap<String, String>,
    type_aliases: &'a BTreeMap<String, Type>,
    forbidden: &'a [String],
    expanding: BTreeSet<String>,
    module: Vec<String>,
    symbols: &'a BTreeMap<Vec<String>, Symbol>,
    found: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for OwnedTypes<'ast> {
    /// References borrow another owner and do not retain authoritative state.
    fn visit_type_reference(&mut self, _: &'ast syn::TypeReference) {}

    /// Raw pointers do not own the pointed-to state either.
    fn visit_type_ptr(&mut self, _: &'ast syn::TypePtr) {}

    /// Finds owned types through generics and aliases, excluding Bevy's borrowed resources.
    fn visit_type_path(&mut self, ty: &'ast syn::TypePath) {
        let Some(last) = ty.path.segments.last() else {
            return;
        };
        let name = resolved_name(&last.ident.to_string(), self.aliases);
        if matches!(name.as_str(), "Res" | "ResMut")
            && matches!(&last.arguments, syn::PathArguments::AngleBracketed(arguments)
                if arguments.args.iter().any(|argument| matches!(argument, syn::GenericArgument::Lifetime(_))))
        {
            return;
        }
        if self.forbidden.contains(&name) {
            self.found.insert(name.clone());
        }
        if let Some(alias) = self
            .type_aliases
            .get(&last.ident.to_string())
            .or_else(|| self.type_aliases.get(&name))
            && self.expanding.insert(name.clone())
        {
            self.visit_type(alias);
            self.expanding.remove(&name);
        }
        let key = symbol_path(
            &self.module,
            &ty.path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
        );
        if let Some(symbol) = self.symbols.get(&key) {
            let expanding = key.join("::");
            if self.expanding.insert(expanding.clone()) {
                let module = std::mem::replace(&mut self.module, symbol.module.clone());
                self.visit_type(&symbol.ty);
                self.module = module;
                self.expanding.remove(&expanding);
            }
        }
        syn::visit::visit_type_path(self, ty);
    }
}
