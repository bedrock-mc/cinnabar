use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};
use syn::{
    Expr, ExprMethodCall, ImplItemFn, ItemFn, Lit, LitStr, Local, Macro, Pat, Token,
    punctuated::Punctuated,
    visit::{self, Visit},
};
use workspace_declarations::{READS, read_names, within};

use crate::{
    ArchitectureError,
    paths::{cinnabar_list, crate_roots, owning_crate, relative_slash},
    policy::Policy,
    read,
};

/// Requires `[package.metadata.cinnabar] reads` to list every repository path outside a crate
/// that it reads, in another crate or in no crate, so CI selects the reader when it changes.
///
/// A read is an `include_str!`/`include_bytes!` path, resolved from the source file, or a
/// literal starting with `../` or `/../`, resolved from the crate root as
/// `env!("CARGO_MANIFEST_DIR")` paths are. Paths in no crate count only if they exist. A bare
/// repository-relative literal such as `"plan.md"` only keeps a declaration current: tests name
/// such paths as data.
pub(super) fn check_reads(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let roots = crate_roots(policy);
    let mut existing = BTreeSet::new();
    for path in files {
        let mut relative = relative_slash(root, path);
        while !relative.is_empty() && existing.insert(relative.clone()) {
            relative.truncate(relative.rfind('/').unwrap_or(0));
        }
    }
    let mut required = BTreeMap::<&str, BTreeMap<String, String>>::new();
    let mut mentioned = BTreeMap::<&str, BTreeSet<String>>::new();
    for path in files {
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let relative = relative_slash(root, path);
        let Some((crate_root, name)) = owning_crate(&roots, &relative) else {
            continue;
        };
        // Unparseable sources are reported by the module checks.
        let Ok(file) = syn::parse_file(&read(path)?) else {
            continue;
        };
        let mut scan = Literals::default();
        scan.visit_file(&file);
        let mut joins = Joins {
            crate_root,
            bindings: BTreeMap::new(),
            targets: Vec::new(),
        };
        joins.visit_file(&file);
        let directory = relative
            .rsplit_once('/')
            .map_or("", |(directory, _)| directory);
        let foreign = |target: Option<String>| {
            target.filter(|target| match owning_crate(&roots, target) {
                Some((owner, _)) => owner != crate_root,
                None => existing.contains(target),
            })
        };
        for target in joins.targets {
            if let Some(target) = foreign(Some(target)) {
                required
                    .entry(name)
                    .or_default()
                    .entry(target)
                    .or_insert_with(|| relative.clone());
            }
        }
        for literal in &scan.included {
            if let Some(target) = foreign(resolve(directory, literal)) {
                required
                    .entry(name)
                    .or_default()
                    .entry(target)
                    .or_insert_with(|| relative.clone());
            }
        }
        for literal in &scan.literals {
            let relative_to_crate = literal
                .strip_prefix('/')
                .unwrap_or(literal)
                .starts_with("../")
                && names_path(literal);
            if relative_to_crate {
                if let Some(target) = foreign(resolve(crate_root, literal.trim_start_matches('/')))
                {
                    required
                        .entry(name)
                        .or_default()
                        .entry(target)
                        .or_insert_with(|| relative.clone());
                }
            } else if let Some(target) = foreign(resolve("", literal)) {
                mentioned.entry(name).or_default().insert(target);
            }
        }
    }
    for (crate_root, name) in &roots {
        let manifest_path = root.join(crate_root).join("Cargo.toml");
        let manifest = toml::from_str::<toml::Value>(&read(&manifest_path)?).map_err(|source| {
            ArchitectureError::Policy {
                path: manifest_path.clone(),
                source,
            }
        })?;
        let declared = cinnabar_list(&manifest, READS)
            .filter_map(toml::Value::as_str)
            .collect::<Vec<_>>();
        let reads = required.remove(name).unwrap_or_default();
        for (target, source) in &reads {
            if !declared.iter().any(|pattern| declares(pattern, target)) {
                diagnostics.push(format!(
                    "{name}: {source} reads `{target}` outside the crate; list it in `reads` under [package.metadata.cinnabar]"
                ));
            }
        }
        let mentions = mentioned.remove(name).unwrap_or_default();
        for pattern in declared {
            if !reads
                .keys()
                .chain(&mentions)
                .any(|target| declares(pattern, target) || within(pattern, target))
            {
                diagnostics.push(format!(
                    "{name}: declared read `{pattern}` matches nothing its sources read"
                ));
            }
        }
    }
    Ok(())
}

/// Whether a declaration names everything a read reaches: the read itself, or a directory or
/// `dir/*` glob holding it. A declaration inside a read directory only keeps itself current.
fn declares(pattern: &str, target: &str) -> bool {
    read_names(pattern, target) || pattern.strip_suffix("/*") == Some(target)
}

/// Whether a relative path names something beyond its `.` and `..` steps.
fn names_path(path: &str) -> bool {
    path.split('/').any(|part| !matches!(part, "" | "." | ".."))
}

/// Joins `path` to a repository-relative `base`, returning `""` for the repository root and
/// `None` when it leaves the repository.
fn resolve(base: &str, path: &str) -> Option<String> {
    let mut parts = base
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// Collects string literals, keeping `include_str!`/`include_bytes!` paths apart.
#[derive(Default)]
struct Literals {
    included: Vec<String>,
    literals: Vec<String>,
}

impl Literals {
    /// Macro bodies are not parsed, so their string literals are read from the tokens.
    fn tokens(&mut self, tokens: TokenStream) {
        for tree in tokens {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Ok(literal) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        self.literals.push(literal.value());
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Literals {
    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        self.literals.push(literal.value());
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        let include = mac.path.is_ident("include_str") || mac.path.is_ident("include_bytes");
        match mac.parse_body::<LitStr>() {
            Ok(path) if include => self.included.push(path.value()),
            _ => self.tokens(mac.tokens.clone()),
        }
        visit::visit_macro(self, mac);
    }
}

/// Follows paths built from `env!("CARGO_MANIFEST_DIR")` through `Path::new`, `PathBuf::from`,
/// `concat!` and `.join("literal")`, including `let` bindings within one function, and records
/// each joined path. Joins of computed values and paths passed between functions stay unknown.
struct Joins<'a> {
    crate_root: &'a str,
    bindings: BTreeMap<String, String>,
    targets: Vec<String>,
}

impl Joins<'_> {
    /// Returns the repository path an expression builds, `""` for the repository root.
    fn path(&self, expression: &Expr) -> Option<String> {
        match expression {
            Expr::Macro(expression) => self.macro_path(&expression.mac),
            Expr::Path(path) => self
                .bindings
                .get(&path.path.get_ident()?.to_string())
                .cloned(),
            Expr::Reference(reference) => self.path(&reference.expr),
            Expr::Paren(inner) => self.path(&inner.expr),
            Expr::Call(call) => {
                let Expr::Path(function) = &*call.func else {
                    return None;
                };
                let mut names = function
                    .path
                    .segments
                    .iter()
                    .rev()
                    .map(|segment| segment.ident.to_string());
                let constructor = matches!(
                    (names.next().as_deref(), names.next().as_deref()),
                    (Some("new"), Some("Path")) | (Some("from"), Some("PathBuf"))
                );
                match call.args.first() {
                    Some(argument) if constructor && call.args.len() == 1 => self.path(argument),
                    _ => None,
                }
            }
            Expr::MethodCall(call) => {
                let base = self.path(&call.receiver)?;
                match call.method.to_string().as_str() {
                    "join" => joined(&base, call),
                    "canonicalize" | "unwrap" | "expect" | "to_path_buf" | "as_path" | "clone"
                    | "to_owned" => Some(base),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Resolves `env!("CARGO_MANIFEST_DIR")` and `concat!` of it with string literals.
    fn macro_path(&self, mac: &Macro) -> Option<String> {
        if mac.path.is_ident("env") {
            let name = mac.parse_body::<LitStr>().ok()?;
            return (name.value() == "CARGO_MANIFEST_DIR").then(|| self.crate_root.to_owned());
        }
        if !mac.path.is_ident("concat") {
            return None;
        }
        let parts = mac
            .parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated)
            .ok()?;
        let mut parts = parts.iter();
        let base = self.path(parts.next()?)?;
        let mut suffix = String::new();
        for part in parts {
            let Expr::Lit(literal) = part else {
                return None;
            };
            let Lit::Str(literal) = &literal.lit else {
                return None;
            };
            suffix.push_str(&literal.value());
        }
        resolve(&base, &suffix)
    }
}

/// Resolves `.join("literal")` against an already resolved base.
fn joined(base: &str, call: &ExprMethodCall) -> Option<String> {
    let Some(Expr::Lit(literal)) = call.args.first() else {
        return None;
    };
    let Lit::Str(literal) = &literal.lit else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    resolve(base, &literal.value())
}

impl<'ast> Visit<'ast> for Joins<'_> {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let outer = std::mem::take(&mut self.bindings);
        visit::visit_item_fn(self, item);
        self.bindings = outer;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        let outer = std::mem::take(&mut self.bindings);
        visit::visit_impl_item_fn(self, item);
        self.bindings = outer;
    }

    fn visit_local(&mut self, local: &'ast Local) {
        visit::visit_local(self, local);
        let pattern = match &local.pat {
            Pat::Type(typed) => &*typed.pat,
            pattern => pattern,
        };
        let Pat::Ident(binding) = pattern else {
            return;
        };
        let name = binding.ident.to_string();
        match local.init.as_ref().and_then(|init| self.path(&init.expr)) {
            Some(path) => {
                self.bindings.insert(name, path);
            }
            None => {
                self.bindings.remove(&name);
            }
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        // Joining only `..` moves to a parent without reading it.
        let named = matches!(
            call.args.first(),
            Some(Expr::Lit(literal)) if matches!(&literal.lit, Lit::Str(path) if names_path(&path.value()))
        );
        if call.method == "join"
            && named
            && let Some(target) = self
                .path(&call.receiver)
                .and_then(|base| joined(&base, call))
        {
            self.targets.push(target);
        }
        visit::visit_expr_method_call(self, call);
    }
}
