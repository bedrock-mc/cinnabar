use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use syn::{punctuated::Punctuated, spanned::Spanned, visit::Visit};

use crate::{ArchitectureError, paths::relative_slash, policy::Policy, read};

const SLEEP_HELP: &str = "tests wait with `test_time::eventually` (Rust) or `testwait` or `testing/synctest` (Go), never sleep";
const CLOCK_HELP: &str = "domain code takes `now` from its caller; only edge code reads the clock";

/// Rejects sleeps in tests and clock reads in clock-free crates' `src/`, except listed exceptions.
pub(super) fn check_timing(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let rules = &policy.timing;
    for exception in &rules.sleep_exceptions {
        if exception.reason.trim().is_empty() {
            diagnostics.push(format!(
                "{}: sleep exception needs a reason",
                exception.path
            ));
        }
    }
    let mut used = vec![false; rules.sleep_exceptions.len()];
    let mut scans = BTreeMap::new();
    for path in files {
        let relative = relative_slash(root, path);
        if relative.ends_with("_test.go") {
            for (index, line) in read(path)?.lines().enumerate() {
                if line
                    .split("//")
                    .next()
                    .unwrap_or("")
                    .contains("time.Sleep(")
                {
                    report_sleep(&relative, index + 1, policy, &mut used, diagnostics);
                }
            }
        } else if relative.ends_with(".rs") {
            match syn::parse_file(&read(path)?) {
                Ok(file) => {
                    let mut scan = RustScan::new(path, is_rust_test(&relative));
                    scan.visit_file(&file);
                    scans.insert(
                        path.canonicalize().unwrap_or_else(|_| path.clone()),
                        (relative, scan),
                    );
                }
                Err(error) => diagnostics.push(format!(
                    "{relative}:{}: cannot parse Rust for timing check: {error}",
                    error.span().start().line
                )),
            }
        }
    }
    // Propagate test scope through every out-of-line declaration, regardless of file order.
    let mut test_files = BTreeSet::new();
    loop {
        let mut changed = false;
        for (path, (_, scan)) in &scans {
            let inherited = test_files.contains(path) || scan.file_test;
            for (module, test) in &scan.modules {
                if (inherited || *test) && test_files.insert(module.clone()) {
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    for (path, (relative, scan)) in scans {
        let file_test = scan.file_test || test_files.contains(&path);
        let clock_free = rules
            .clock_free_crates
            .iter()
            .any(|krate| relative.starts_with(&format!("{krate}/src/")));
        for (line, test, sleep) in scan.calls {
            if sleep && (file_test || test) {
                report_sleep(&relative, line, policy, &mut used, diagnostics);
            } else if !sleep && clock_free && !file_test && !test {
                diagnostics.push(format!("{relative}:{line}: {CLOCK_HELP}"));
            }
        }
    }
    for (exception, used) in rules.sleep_exceptions.iter().zip(used) {
        if !used {
            diagnostics.push(format!(
                "{}: stale sleep exception; the file no longer sleeps",
                exception.path
            ));
        }
    }
    Ok(())
}

/// Records a test sleep or marks its file's explicit exception as used.
fn report_sleep(
    relative: &str,
    line: usize,
    policy: &Policy,
    used: &mut [bool],
    diagnostics: &mut Vec<String>,
) {
    match policy
        .timing
        .sleep_exceptions
        .iter()
        .position(|entry| entry.path == relative)
    {
        Some(index) => used[index] = true,
        None => diagnostics.push(format!("{relative}:{line}: {SLEEP_HELP}")),
    }
}

/// Test files and test-only directories, including `foo_tests/` helper modules, by repository-relative path.
fn is_rust_test(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or("");
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || relative
            .split('/')
            .rev()
            .skip(1)
            .any(|dir| dir == "tests" || dir.ends_with("_tests"))
}

/// Whether an attribute excludes every configuration without `test`.
fn test_attributes(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| requires_test(&meta))
    })
}

/// Recognizes `test`, conjunctions containing it, and disjunctions requiring it in every arm.
fn requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) => {
            let Ok(arms) =
                list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            else {
                return false;
            };
            (list.path.is_ident("all") && arms.iter().any(requires_test))
                || (list.path.is_ident("any") && !arms.is_empty() && arms.iter().all(requires_test))
        }
        _ => false,
    }
}

/// Attributes on Rust items, including functions, modules, and impl blocks.
fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
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

/// Collects imported paths, expanding groups, aliases, and the two supported sleep globs.
fn import_paths(
    tree: &syn::UseTree,
    prefix: Vec<String>,
    imports: &mut BTreeMap<String, Vec<String>>,
) {
    match tree {
        syn::UseTree::Path(path) => {
            let mut prefix = prefix;
            prefix.push(path.ident.to_string());
            import_paths(&path.tree, prefix, imports);
        }
        syn::UseTree::Name(name) => {
            let mut path = prefix;
            if name.ident != "self" {
                path.push(name.ident.to_string());
            }
            if let Some(name) = path.last() {
                imports.insert(name.clone(), path);
            }
        }
        syn::UseTree::Rename(rename) => {
            let mut path = prefix;
            if rename.ident != "self" {
                path.push(rename.ident.to_string());
            }
            imports.insert(rename.rename.to_string(), path);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                import_paths(tree, prefix.clone(), imports);
            }
        }
        syn::UseTree::Glob(_) => {
            if prefix == ["std", "thread"] || prefix == ["tokio", "time"] {
                let mut path = prefix;
                path.push("sleep".into());
                imports.insert("sleep".into(), path);
            }
        }
    }
}

/// Parsed calls retain their item scope so module inheritance can be applied after all files are read.
struct RustScan {
    file_test: bool,
    in_test: bool,
    module_dir: PathBuf,
    path_dir: PathBuf,
    imports: BTreeMap<String, Vec<String>>,
    calls: Vec<(usize, bool, bool)>,
    modules: Vec<(PathBuf, bool)>,
}

impl RustScan {
    /// Starts a file scan using Rust's module directory rules.
    fn new(path: &Path, file_test: bool) -> Self {
        let parent = path.parent().unwrap_or(Path::new("."));
        let stem = path.file_stem().unwrap_or_default();
        let module_dir = if ["lib", "main", "mod"].iter().any(|name| stem == *name) {
            parent.to_path_buf()
        } else {
            parent.join(stem)
        };
        Self {
            file_test,
            in_test: file_test,
            module_dir,
            path_dir: parent.to_path_buf(),
            imports: BTreeMap::new(),
            calls: Vec::new(),
            modules: Vec::new(),
        }
    }

    /// Loads item imports before visiting calls, since Rust imports are independent of item order.
    fn collect_imports<'a>(&mut self, items: impl Iterator<Item = &'a syn::Item>) {
        for item in items {
            if let syn::Item::Use(item) = item {
                import_paths(&item.tree, Vec::new(), &mut self.imports);
            }
        }
    }
}

impl<'ast> Visit<'ast> for RustScan {
    /// Applies file-level test attributes before visiting its imports and items.
    fn visit_file(&mut self, file: &'ast syn::File) {
        self.file_test |= test_attributes(&file.attrs);
        self.in_test = self.file_test;
        self.collect_imports(file.items.iter());
        syn::visit::visit_file(self, file);
    }

    /// Keeps cfg-based test scope inside the attributed item.
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let previous = self.in_test;
        self.in_test |= test_attributes(item_attributes(item));
        syn::visit::visit_item(self, item);
        self.in_test = previous;
    }

    /// Applies test scope to individual impl members without affecting their siblings.
    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attrs = match item {
            syn::ImplItem::Const(item) => &item.attrs[..],
            syn::ImplItem::Fn(item) => &item.attrs[..],
            syn::ImplItem::Type(item) => &item.attrs[..],
            syn::ImplItem::Macro(item) => &item.attrs[..],
            _ => &[],
        };
        let previous = self.in_test;
        self.in_test |= test_attributes(attrs);
        syn::visit::visit_impl_item(self, item);
        self.in_test = previous;
    }

    /// Applies test scope to individual trait members without affecting their siblings.
    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        let attrs = match item {
            syn::TraitItem::Const(item) => &item.attrs[..],
            syn::TraitItem::Fn(item) => &item.attrs[..],
            syn::TraitItem::Type(item) => &item.attrs[..],
            syn::TraitItem::Macro(item) => &item.attrs[..],
            _ => &[],
        };
        let previous = self.in_test;
        self.in_test |= test_attributes(attrs);
        syn::visit::visit_trait_item(self, item);
        self.in_test = previous;
    }

    /// Scans inline modules and records resolved files for out-of-line declarations.
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if let Some((_, items)) = &module.content {
            let old_dir = self.module_dir.clone();
            let old_path_dir = self.path_dir.clone();
            let old_imports = self.imports.clone();
            self.module_dir.push(module.ident.to_string());
            self.path_dir = self.module_dir.clone();
            self.collect_imports(items.iter());
            syn::visit::visit_item_mod(self, module);
            self.module_dir = old_dir;
            self.path_dir = old_path_dir;
            self.imports = old_imports;
        } else {
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
            if let Some(path) = candidates.into_iter().find(|path| path.is_file()) {
                self.modules
                    .push((path.canonicalize().unwrap_or(path), self.in_test));
            }
        }
    }

    /// Makes block-local imports visible only within that block.
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let previous = self.imports.clone();
        self.collect_imports(block.stmts.iter().filter_map(|statement| match statement {
            syn::Stmt::Item(item) => Some(item),
            _ => None,
        }));
        syn::visit::visit_block(self, block);
        self.imports = previous;
    }

    /// Records supported sleep and clock calls with their span line and current test scope.
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func {
            let mut path: Vec<_> = function
                .path
                .segments
                .iter()
                .map(|part| part.ident.to_string())
                .collect();
            if let Some(import) = path.first().and_then(|name| self.imports.get(name)) {
                path = import
                    .iter()
                    .cloned()
                    .chain(path.into_iter().skip(1))
                    .collect();
            }
            let sleep = path == ["std", "thread", "sleep"]
                || path == ["thread", "sleep"]
                || path == ["tokio", "time", "sleep"];
            let clock = path.len() >= 2
                && path.last().is_some_and(|name| name == "now")
                && matches!(path[path.len() - 2].as_str(), "Instant" | "SystemTime");
            if sleep || clock {
                self.calls
                    .push((call.span().start().line, self.in_test, sleep));
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the timing gate on a temporary repository with a clock-free fixture crate.
    fn diagnostics(sources: &[(&str, &str)]) -> Vec<String> {
        let root = tempfile::tempdir().unwrap();
        let files: Vec<_> = sources
            .iter()
            .map(|(name, source)| {
                let path = root.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, source).unwrap();
                path
            })
            .collect();
        let policy = toml::from_str::<Policy>(
            "production_rust_max = 1000\nmodule_root_max = 1000\npowershell_max = 1000\ntest_max = 1000\n[timing]\nclock_free_crates = ['crates/a']"
        ).unwrap();
        let mut diagnostics = Vec::new();
        check_timing(root.path(), &policy, &files, &mut diagnostics).unwrap();
        diagnostics.sort();
        diagnostics
    }

    #[test]
    fn test_files_and_helper_directories_count_as_tests() {
        for path in [
            "crates/a/src/tests.rs",
            "crates/a/src/foo_tests.rs",
            "crates/a/tests/it/main.rs",
            "crates/a/src/foo_tests/carrier.rs",
        ] {
            assert!(is_rust_test(path), "{path}");
        }
        assert!(!is_rust_test("crates/a/src/contests.rs"));
        assert!(!is_rust_test("crates/a/src/latest/mod.rs"));
    }

    #[test]
    fn braces_strings_and_comments_do_not_change_scope_or_count_as_calls() {
        let found = diagnostics(&[(
            "crates/a/src/lib.rs",
            r#"
#[cfg(test)] fn helper() { let text = "{"; }
fn live() {
    let text = "std::thread::sleep(Instant::now())";
    // std::thread::sleep(Instant::now());
    Instant::now();
}
"#,
        )]);
        assert_eq!(found, [format!("crates/a/src/lib.rs:6: {CLOCK_HELP}")]);
    }

    #[test]
    fn multiline_test_function_allows_clocks_and_rejects_sleep() {
        let found = diagnostics(&[(
            "crates/a/src/lib.rs",
            "#[cfg(test)]\nfn helper(\n    a: u32,\n    b: u32,\n) {\n    Instant::now();\n    std::thread::sleep(Duration::ZERO);\n}\nfn live() { SystemTime::now(); }",
        )]);
        assert_eq!(
            found,
            [
                format!("crates/a/src/lib.rs:7: {SLEEP_HELP}"),
                format!("crates/a/src/lib.rs:9: {CLOCK_HELP}")
            ]
        );
    }

    #[test]
    fn cfg_predicates_must_require_test_in_every_alternative() {
        let found = diagnostics(&[(
            "crates/a/src/lib.rs",
            "#[cfg(all(test, unix))] fn a() { Instant::now(); }\n#[cfg(any(test, unix))] fn b() { Instant::now(); }\n#[cfg(not(test))] fn c() { SystemTime::now(); }\n#[cfg(any(test, all(test, unix)))] fn d() { Instant::now(); }",
        )]);
        assert_eq!(
            found,
            [
                format!("crates/a/src/lib.rs:2: {CLOCK_HELP}"),
                format!("crates/a/src/lib.rs:3: {CLOCK_HELP}")
            ]
        );
    }

    #[test]
    fn out_of_line_test_scope_follows_nested_modules_and_explicit_paths() {
        let found = diagnostics(&[
            ("crates/a/src/lib.rs", "mod forms;\nmod ordinary;"),
            (
                "crates/a/src/forms.rs",
                "#[cfg(test)] mod snapshots;\n#[cfg(test)] #[path = \"support.rs\"] mod helpers;\n#[cfg(test)] mod inline { mod child; }",
            ),
            (
                "crates/a/src/forms/snapshots.rs",
                "mod nested;\nfn helper() { std::thread::sleep(Duration::ZERO); Instant::now(); }",
            ),
            (
                "crates/a/src/forms/snapshots/nested/mod.rs",
                "fn helper() { thread::sleep(Duration::ZERO); SystemTime::now(); }",
            ),
            (
                "crates/a/src/support.rs",
                "fn helper() { thread::sleep(Duration::ZERO); Instant::now(); }",
            ),
            (
                "crates/a/src/forms/inline/child.rs",
                "fn helper() { thread::sleep(Duration::ZERO); Instant::now(); }",
            ),
            ("crates/a/src/ordinary.rs", "fn live() { Instant::now(); }"),
        ]);
        assert_eq!(
            found,
            [
                format!("crates/a/src/forms/inline/child.rs:1: {SLEEP_HELP}"),
                format!("crates/a/src/forms/snapshots.rs:2: {SLEEP_HELP}"),
                format!("crates/a/src/forms/snapshots/nested/mod.rs:1: {SLEEP_HELP}"),
                format!("crates/a/src/ordinary.rs:1: {CLOCK_HELP}"),
                format!("crates/a/src/support.rs:1: {SLEEP_HELP}"),
            ]
        );
    }

    #[test]
    fn tokio_and_imported_sleeps_are_calls_even_when_awaited() {
        let found = diagnostics(&[(
            "crates/a/tests/async.rs",
            "use std::thread::{self, sleep};\nuse tokio::time;\nasync fn helper() {\n    tokio::time::sleep(Duration::ZERO).await;\n    time::sleep(Duration::ZERO).await;\n    sleep(Duration::ZERO);\n    thread::sleep(Duration::ZERO);\n    { use tokio::time::sleep as pause; pause(Duration::ZERO).await; }\n    pause(Duration::ZERO);\n}",
        )]);
        assert_eq!(
            found,
            (4..=8)
                .map(|line| format!("crates/a/tests/async.rs:{line}: {SLEEP_HELP}"))
                .collect::<Vec<_>>()
        );
        assert!(
            diagnostics(&[(
                "crates/a/tests/other.rs",
                "use other::time; fn helper() { time::sleep(); sleep(); }"
            )])
            .is_empty()
        );
    }

    #[test]
    fn test_impl_and_trait_members_have_their_own_scope() {
        let found = diagnostics(&[(
            "crates/a/src/lib.rs",
            "impl A { #[cfg(test)] fn helper() { Instant::now(); thread::sleep(Duration::ZERO); } fn live() { Instant::now(); } }\ntrait B { #[cfg(test)] fn helper() { SystemTime::now(); } }",
        )]);
        assert_eq!(
            found,
            [
                format!("crates/a/src/lib.rs:1: {SLEEP_HELP}"),
                format!("crates/a/src/lib.rs:1: {CLOCK_HELP}")
            ]
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_errors_are_file_diagnostics() {
        let found = diagnostics(&[("crates/a/src/broken.rs", "fn broken( {")]);
        assert_eq!(found.len(), 1);
        assert!(
            found[0].starts_with("crates/a/src/broken.rs:1: cannot parse Rust for timing check:")
        );
    }
}
