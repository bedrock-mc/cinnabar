//! `mod.toml`, a player mod package's manifest: the one parser both the host and a mod's build
//! script ([`crate::declarations::generate_mod`]) read it with.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// The package path of the mod's component.
pub const COMPONENT: &str = "mod.wasm";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModPermission {
    Screen,
    Items,
    Recipes,
    Keys,
    Inventory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyDecl {
    pub id: String,
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModManifest {
    pub id: String,
    pub version: String,
    pub api: String,
    pub permissions: BTreeSet<ModPermission>,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub textures: Vec<String>,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub keys: Vec<KeyDecl>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

/// The `api` a package declares: the `cinnabar:extension` package version its `player-mod` component is built against.
pub const API: &str = "0.1";
/// The images a package's screens may draw live under this directory.
pub const TEXTURE_DIR: &str = "textures/";

impl ModManifest {
    /// Parses and checks `mod.toml`: every template, texture and the component has a SHA-256 in
    /// `[files]` and nothing else does, every path stays inside the package, and every action
    /// and key is in the mod's namespace and declared once.
    pub fn parse(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        manifest.check()?;
        manifest.check_files()?;
        Ok(manifest)
    }

    /// Parses and checks `mod.toml` as [`parse`](Self::parse) does but `[files]`, which a
    /// package script fills only after the component is built.
    pub fn parse_declarations(text: &str) -> Result<Self, String> {
        let manifest: Self = toml::from_str(text).map_err(|error| error.to_string())?;
        manifest.check()?;
        Ok(manifest)
    }

    /// The JSON-UI namespace the mod's templates declare, and the prefix of its actions and
    /// keys: its id.
    pub fn namespace(&self) -> String {
        self.id.clone()
    }

    /// The SHA-256 (lowercase hex) `[files]` gives `path`.
    pub fn hash(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    fn check(&self) -> Result<(), String> {
        let name = |text: &str| {
            !text.is_empty()
                && text.len() <= 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        };
        if !name(&self.id) || self.id.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!("id {:?} is not a lowercase name", self.id));
        }
        if self.api != API {
            return Err(format!("api {} is not {API}", self.api));
        }
        for template in &self.templates {
            let root = template
                .strip_prefix("ui/")
                .and_then(|path| path.strip_suffix(".json"));
            if !root.is_some_and(name) {
                return Err(format!("template {template} is not ui/<name>.json"));
            }
        }
        for texture in &self.textures {
            let inside = texture.strip_prefix(TEXTURE_DIR).is_some_and(|path| {
                path.split('/').all(|part| {
                    !part.is_empty()
                        && part != ".."
                        && part != "."
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                })
            });
            if !inside || !(texture.ends_with(".png") || texture.ends_with(".json")) {
                return Err(format!("texture {texture} is not a textures/ image"));
            }
        }
        let prefix = format!("{}.", self.id);
        let owned = |id: &str| {
            id.strip_prefix(&prefix)
                .is_some_and(|rest| !rest.is_empty())
        };
        let mut actions = BTreeSet::new();
        for action in &self.actions {
            if !owned(action) || !actions.insert(action.as_str()) {
                return Err(format!(
                    "action {action} is outside {prefix} or declared twice"
                ));
            }
        }
        let (mut ids, mut bindings) = (BTreeSet::new(), BTreeSet::new());
        for key in &self.keys {
            if !owned(&key.id) || !ids.insert(key.id.as_str()) {
                return Err(format!(
                    "key {} is outside {prefix} or declared twice",
                    key.id
                ));
            }
            if !KEY_NAMES.contains(&key.key.as_str()) {
                return Err(format!(
                    "key {} binds {}, which is not a key name",
                    key.id, key.key
                ));
            }
            let mut modifiers = key.modifiers.clone();
            modifiers.sort();
            modifiers.dedup();
            if modifiers.len() != key.modifiers.len()
                || !bindings.insert((key.key.as_str(), modifiers))
            {
                return Err(format!("key {} repeats a binding", key.id));
            }
            if key.label.is_empty() || key.label.chars().any(char::is_control) {
                return Err(format!("key {} needs a label", key.id));
            }
        }
        Ok(())
    }
}

impl ModManifest {
    /// Every template, texture and the component has a SHA-256 in `[files]`, and nothing else
    /// does.
    fn check_files(&self) -> Result<(), String> {
        let declared: BTreeSet<&str> = self
            .templates
            .iter()
            .chain(&self.textures)
            .map(String::as_str)
            .chain([COMPONENT])
            .collect();
        if let Some(path) = declared
            .iter()
            .find(|path| !self.files.contains_key(**path))
        {
            return Err(format!("{path} has no hash in [files]"));
        }
        for (path, hash) in &self.files {
            if !declared.contains(path.as_str()) {
                return Err(format!("{path} is in [files] but not declared"));
            }
            if hash.len() != 64 || !hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(format!("{path}'s hash is not a lowercase SHA-256"));
            }
        }
        Ok(())
    }
}

/// The keys a declaration may bind, by lowercase name: letters, digits, F1 to F12, Backspace,
/// Page Up and Page Down.
pub const KEY_NAMES: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "backspace",
    "page_up",
    "page_down",
];

#[cfg(test)]
mod tests;
