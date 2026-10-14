use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use syn::punctuated::Punctuated;

/// Test files and test-only directories, including `foo_tests/` helper modules, by repository-relative path.
pub(super) fn is_rust_test(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || relative
            .split('/')
            .rev()
            .skip(1)
            .any(|dir| dir == "tests" || dir.ends_with("_tests"))
}

/// Recognizes test entry attributes and cfg predicates that require `test` or a registered test-support feature.
pub(super) fn marks_test_scope(attributes: &[syn::Attribute], features: &BTreeSet<String>) -> bool {
    attributes.iter().any(|attribute| {
        attribute
            .path()
            .segments
            .last()
            .is_some_and(|part| part.ident == "test")
            || (attribute.path().is_ident("cfg")
                && attribute
                    .parse_args::<syn::Meta>()
                    .is_ok_and(|meta| requires_test(&meta, features)))
    })
}

/// Recognizes `test` or a test-support feature, conjunctions containing one, and disjunctions of only those.
fn requires_test(meta: &syn::Meta, features: &BTreeSet<String>) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::NameValue(pair) => {
            pair.path.is_ident("feature")
                && matches!(&pair.value, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. })
                    if features.contains(&name.value()))
        }
        syn::Meta::List(list) => {
            let Ok(arms) =
                list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            else {
                return false;
            };
            let test = |arm: &syn::Meta| requires_test(arm, features);
            (list.path.is_ident("all") && arms.iter().any(test))
                || (list.path.is_ident("any") && !arms.is_empty() && arms.iter().all(test))
        }
    }
}

/// Attributes on Rust items, including functions, modules, and impl blocks.
pub(super) fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

/// Resolves module files using the source file and enclosing inline modules.
#[derive(Clone)]
pub(super) struct ModulePaths {
    module_dir: PathBuf,
    path_dir: PathBuf,
}

impl ModulePaths {
    /// Starts an entrypoint's module directories beside the crate root source file.
    pub(super) fn crate_root(path: &Path) -> Self {
        let parent = path.parent().unwrap_or(Path::new("."));
        Self {
            module_dir: parent.to_path_buf(),
            path_dir: parent.to_path_buf(),
        }
    }

    /// Starts Rust's default module directory and explicit-path directory.
    pub(super) fn new(path: &Path) -> Self {
        let parent = path.parent().unwrap_or(Path::new("."));
        let stem = path.file_stem().unwrap_or_default();
        let module_dir = if ["lib", "main", "mod"].iter().any(|name| stem == *name) {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        Self {
            module_dir,
            path_dir: parent.to_path_buf(),
        }
    }

    /// Descends into an inline module for subsequent child declarations.
    pub(super) fn push_inline(&mut self, name: &str) {
        self.module_dir.push(name);
        self.path_dir = self.module_dir.clone();
    }

    /// Returns an existing out-of-line module file, including explicit `path` attributes.
    pub(super) fn resolve(&self, module: &syn::ItemMod) -> Option<PathBuf> {
        let explicit = module.attrs.iter().find_map(|attribute| {
            if attribute.path().is_ident("path")
                && let syn::Meta::NameValue(value) = &attribute.meta
                && let syn::Expr::Lit(literal) = &value.value
                && let syn::Lit::Str(path) = &literal.lit
            {
                return Some(self.path_dir.join(path.value()));
            }
            None
        });
        let candidates = match explicit {
            Some(path) => vec![path],
            None => vec![
                self.module_dir.join(format!("{}.rs", module.ident)),
                self.module_dir
                    .join(module.ident.to_string())
                    .join("mod.rs"),
            ],
        };
        candidates
            .into_iter()
            .find(|path| path.is_file())
            .map(|path| path.canonicalize().unwrap_or(path))
    }
}

/// Propagates test-only scope through out-of-line modules, independent of file order.
pub(super) fn inherited_test_files<'a>(
    scans: impl Iterator<Item = (&'a PathBuf, bool, &'a [(PathBuf, bool)])> + Clone,
) -> BTreeSet<PathBuf> {
    let mut test_files = BTreeSet::new();
    loop {
        let mut changed = false;
        for (path, file_test, modules) in scans.clone() {
            let inherited = test_files.contains(path) || file_test;
            for (module, test) in modules {
                if (inherited || *test) && test_files.insert(module.clone()) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    test_files
}

/// Finds files reached from production entrypoints without following test-only module edges.
pub(super) fn reachable_production_files<'a>(
    scans: impl Iterator<Item = (&'a PathBuf, bool, &'a [(PathBuf, bool)])> + Clone,
    roots: impl IntoIterator<Item = PathBuf>,
) -> BTreeSet<PathBuf> {
    let mut production_files: BTreeSet<_> = roots.into_iter().collect();
    loop {
        let mut changed = false;
        for (path, file_test, modules) in scans.clone() {
            if file_test || !production_files.contains(path) {
                continue;
            }
            for (module, test) in modules {
                if !test && production_files.insert(module.clone()) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    production_files
}
