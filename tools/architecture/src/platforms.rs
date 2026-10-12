use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::{
    Attribute, File, ItemMod, ItemUse, Macro, Meta, UseTree,
    ext::IdentExt,
    visit::{self, Visit},
};
use workspace_declarations::{MACOS, PLATFORM_SPECIFIC, PLATFORMS, WINDOWS};

use crate::{
    ArchitectureError,
    conditions::{confined_to, target_predicates},
    paths::{cinnabar_list, crate_roots, owning_crate, relative_slash},
    policy::Policy,
    read,
};

/// A platform a crate declares in `[package.metadata.cinnabar] platform-specific`, which CI
/// reads to decide which runners a change needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Platform {
    Macos,
    Windows,
}

// Every declarable platform has a variant.
const _: () = assert!(Platform::ALL.len() == PLATFORMS.len());

impl Platform {
    const ALL: [Self; 2] = [Self::Macos, Self::Windows];

    fn name(self) -> &'static str {
        match self {
            Self::Macos => MACOS,
            Self::Windows => WINDOWS,
        }
    }

    /// The cfg atom every built-in target of a platform-specific predicate must satisfy.
    fn scope(self) -> &'static str {
        match self {
            Self::Macos => "target_vendor=\"apple\"",
            Self::Windows => "windows",
        }
    }

    /// Whether a crate, by Cargo or Rust name, exists only to reach this platform's APIs.
    fn owns_crate(self, name: &str) -> bool {
        let name = name.replace('-', "_");
        match self {
            Self::Macos => name.starts_with("objc2") || name == "metal",
            Self::Windows => matches!(name.as_str(), "windows" | "windows_sys" | "winapi"),
        }
    }
}

/// Requires each crate's declared platforms to match the platform-specific code it contains.
pub(super) fn check_platforms(
    root: &Path,
    policy: &Policy,
    files: &[PathBuf],
    diagnostics: &mut Vec<String>,
) -> Result<(), ArchitectureError> {
    let roots = crate_roots(policy);
    let mut evidence = BTreeMap::<&str, BTreeMap<Platform, String>>::new();
    for path in files {
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }
        let relative = relative_slash(root, path);
        let Some((_, name)) = owning_crate(&roots, &relative) else {
            continue;
        };
        // Unparseable sources are reported by the module checks.
        let Ok(file) = syn::parse_file(&read(path)?) else {
            continue;
        };
        let mut scan = Scan {
            modules: local_modules(&file),
            ..Scan::default()
        };
        scan.visit_file(&file);
        for platform in scan.found {
            evidence
                .entry(name)
                .or_default()
                .entry(platform)
                .or_insert_with(|| relative.clone());
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
        let mut scan = Scan::default();
        scan.manifest(&manifest);
        let found = evidence.entry(*name).or_default();
        for platform in scan.found {
            found
                .entry(platform)
                .or_insert_with(|| relative_slash(root, &manifest_path));
        }
        let declared = declared_platforms(name, &manifest, diagnostics);
        for platform in Platform::ALL {
            match (found.get(&platform), declared.contains(&platform)) {
                (Some(source), false) => diagnostics.push(format!(
                    "{name}: {} code in {source} needs `platform-specific = [\"{}\"]` under [package.metadata.cinnabar]",
                    platform.name(),
                    platform.name()
                )),
                (None, true) => diagnostics.push(format!(
                    "{name}: declares platform-specific \"{}\" but has no {} code",
                    platform.name(),
                    platform.name()
                )),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Reads `[package.metadata.cinnabar] platform-specific`, reporting values it does not know.
fn declared_platforms(
    name: &str,
    manifest: &toml::Value,
    diagnostics: &mut Vec<String>,
) -> BTreeSet<Platform> {
    let mut declared = BTreeSet::new();
    for value in cinnabar_list(manifest, PLATFORM_SPECIFIC) {
        match Platform::ALL
            .into_iter()
            .find(|platform| value.as_str() == Some(platform.name()))
        {
            Some(platform) => {
                declared.insert(platform);
            }
            None => diagnostics.push(format!(
                "{name}: unknown platform-specific value {value}; expected one of {PLATFORMS:?}"
            )),
        }
    }
    declared
}

/// Collects the platforms a source file or manifest singles out.
#[derive(Default)]
struct Scan {
    found: BTreeSet<Platform>,
    modules: BTreeSet<String>, // the file's own modules, which shadow same-named crates
}

/// Returns the names of every module `file` declares.
fn local_modules(file: &File) -> BTreeSet<String> {
    #[derive(Default)]
    struct Modules(BTreeSet<String>);
    impl<'ast> Visit<'ast> for Modules {
        fn visit_item_mod(&mut self, item: &'ast ItemMod) {
            self.0.insert(item.ident.unraw().to_string());
            visit::visit_item_mod(self, item);
        }
    }
    let mut modules = Modules::default();
    modules.visit_file(file);
    modules.0
}

impl Scan {
    /// Records platforms whose targets alone satisfy a predicate the condition tests.
    fn condition(&mut self, meta: &Meta, bare: bool) {
        for predicate in target_predicates(meta, bare) {
            for platform in Platform::ALL {
                if confined_to(&predicate, platform.scope()) {
                    self.found.insert(platform);
                }
            }
        }
    }

    /// Records paths into platform-only crates, `std::os::windows`, and AppKit handles.
    fn path(&mut self, segments: &[String]) {
        let Some(first) = segments.first() else {
            return;
        };
        if segments.len() > 1 {
            for platform in Platform::ALL {
                if platform.owns_crate(first) && !self.modules.contains(first) {
                    self.found.insert(platform);
                }
            }
            if segments
                .windows(2)
                .any(|pair| pair[0] == "os" && pair[1] == "windows")
            {
                self.found.insert(Platform::Windows);
            }
        }
        if segments.iter().any(|segment| segment == "AppKit") {
            self.found.insert(Platform::Macos);
        }
    }

    fn use_tree(&mut self, tree: &UseTree, prefix: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.unraw().to_string());
                self.use_tree(&path.tree, prefix);
                prefix.pop();
            }
            UseTree::Name(name) => {
                prefix.push(name.ident.unraw().to_string());
                self.path(prefix);
                prefix.pop();
            }
            UseTree::Rename(rename) => {
                prefix.push(rename.ident.unraw().to_string());
                self.path(prefix);
                prefix.pop();
            }
            UseTree::Glob(_) => self.path(prefix),
            UseTree::Group(group) => {
                for item in &group.items {
                    self.use_tree(item, prefix);
                }
            }
        }
    }

    /// Macro bodies are not parsed, so their attributes, `cfg!` calls and paths are read as tokens.
    fn tokens(&mut self, tokens: TokenStream) {
        let trees = tokens.into_iter().collect::<Vec<_>>();
        let mut segments = Vec::new();
        let mut colons = 0;
        for (index, tree) in trees.iter().enumerate() {
            match tree {
                TokenTree::Ident(ident) => {
                    if colons < 2 {
                        self.path(&segments);
                        segments.clear();
                    }
                    segments.push(ident.unraw().to_string());
                    colons = 0;
                    continue;
                }
                TokenTree::Punct(punct) if punct.as_char() == ':' => {
                    colons += 1;
                    continue;
                }
                TokenTree::Group(group) => {
                    let before = |offset: usize| index.checked_sub(offset).map(|at| &trees[at]);
                    let attribute = group.delimiter() == Delimiter::Bracket
                        && (is_punct(before(1), '#')
                            || (is_punct(before(1), '!') && is_punct(before(2), '#')));
                    let cfg_call = group.delimiter() == Delimiter::Parenthesis
                        && is_punct(before(1), '!')
                        && matches!(before(2), Some(TokenTree::Ident(ident)) if ident == "cfg");
                    if (attribute || cfg_call)
                        && let Ok(meta) = syn::parse2::<Meta>(group.stream())
                    {
                        self.condition(&meta, cfg_call);
                    }
                    self.tokens(group.stream());
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
            self.path(&segments);
            segments.clear();
            colons = 0;
        }
        self.path(&segments);
    }

    /// Reads target-table cfgs and platform-only dependencies from every dependency table.
    fn manifest(&mut self, manifest: &toml::Value) {
        let targets = manifest.get("target").and_then(toml::Value::as_table);
        for key in targets.into_iter().flat_map(|targets| targets.keys()) {
            if let Ok(meta) = syn::parse_str::<Meta>(key) {
                self.condition(&meta, false);
            }
        }
        let tables = std::iter::once(manifest)
            .chain(targets.into_iter().flat_map(|targets| targets.values()));
        for table in tables {
            for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
                let dependencies = table.get(kind).and_then(toml::Value::as_table);
                for (key, value) in dependencies.into_iter().flatten() {
                    let package = value
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(key.as_str());
                    for platform in Platform::ALL {
                        if platform.owns_crate(package) {
                            self.found.insert(platform);
                        }
                    }
                }
            }
        }
    }
}

fn is_punct(tree: Option<&TokenTree>, expected: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(punct)) if punct.as_char() == expected)
}

impl<'ast> Visit<'ast> for Scan {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.condition(&attribute.meta, false);
        visit::visit_attribute(self, attribute);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        if mac.path.is_ident("cfg") {
            if let Ok(meta) = mac.parse_body::<Meta>() {
                self.condition(&meta, true);
            }
        } else {
            self.tokens(mac.tokens.clone());
        }
        visit::visit_macro(self, mac);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.unraw().to_string())
            .collect::<Vec<_>>();
        self.path(&segments);
        visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.use_tree(&item.tree, &mut Vec::new());
        visit::visit_item_use(self, item);
    }
}
