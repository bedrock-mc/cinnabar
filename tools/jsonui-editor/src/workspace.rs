//! The layered pack set: vanilla at the bottom and resource packs above, each a
//! map of pack-relative files, merged into one catalog the way the client does
//! (the bottom layer by its `_ui_defs.json` load order, every pack above as an
//! overlay). Catalogs are cached per layer prefix so an edit rebuilds only from
//! its own layer up.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};
use std::sync::Arc;

use json_ui::Catalog;

const UI_DEFS: &str = "ui/_ui_defs.json";
const GLOBALS: &str = "ui/_global_variables.json";
const LANG: &str = "texts/en_US.lang";
const SCRATCH: &str = "scratch";
/// What "New file" starts from; `{namespace}` becomes the file's stem.
const SCRATCH_TEMPLATE: &str = r#"{
  "namespace": "{namespace}",

  "main_screen": {
    "type": "screen",
    "controls": [
      {
        "hello": {
          "type": "label",
          "text": "Hello, JSON-UI",
          "shadow": true
        }
      }
    ]
  }
}
"#;
/// Largest single file read out of an archive.
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// One pack in the stack.
#[derive(Default)]
pub struct Layer {
    pub name: String,
    files: BTreeMap<String, Arc<[u8]>>,
    /// Files known to exist whose bytes the host has not supplied yet.
    pending: BTreeSet<String>,
    edited: BTreeSet<String>,
    /// Each edited file's bytes before its first edit; `None` for a new file.
    originals: BTreeMap<String, Option<Arc<[u8]>>>,
    archive: Option<zip::ZipArchive<Cursor<Arc<[u8]>>>>,
    archive_paths: BTreeMap<String, String>,
    generation: u64,
    /// The in-memory layer pasted and new files go to; it stays on top.
    scratch: bool,
}

impl Layer {
    pub fn file(&self, path: &str) -> Option<&Arc<[u8]>> {
        self.files.get(path)
    }

    pub fn text(&self, path: &str) -> Option<String> {
        self.files
            .get(path)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    /// Pack-relative `ui/` json paths, sorted.
    pub fn ui_paths(&self) -> impl Iterator<Item = &str> {
        self.files
            .keys()
            .map(String::as_str)
            .filter(|path| is_ui_json(path))
    }

    pub fn is_scratch(&self) -> bool {
        self.scratch
    }

    pub fn is_edited(&self, path: &str) -> bool {
        self.edited.contains(path)
    }

    pub fn edited_paths(&self) -> impl Iterator<Item = &str> {
        self.edited.iter().map(String::as_str)
    }

    /// An edited file's bytes before editing: `Some(None)` when the edit made it.
    pub fn original(&self, path: &str) -> Option<Option<&Arc<[u8]>>> {
        self.originals.get(path).map(Option::as_ref)
    }

    /// Every file the layer holds, loaded, archived or pending.
    pub fn all_paths(&self) -> BTreeSet<String> {
        self.files
            .keys()
            .chain(self.archive_paths.keys())
            .chain(self.pending.iter())
            .cloned()
            .collect()
    }

    /// Whether the unedited layer has `path`.
    pub fn had(&self, path: &str) -> bool {
        match self.originals.get(path) {
            Some(original) => original.is_some(),
            None => self.has(path),
        }
    }

    fn has(&self, path: &str) -> bool {
        self.files.contains_key(path)
            || self.pending.contains(path)
            || self.archive_paths.contains_key(path)
    }

    fn ui_files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files
            .iter()
            .filter(|(path, _)| is_ui_json(path))
            .map(|(path, bytes)| (path.as_str(), bytes.as_ref()))
    }

    /// Reads `path` out of the archive into the loaded files.
    fn extract(&mut self, path: &str) -> bool {
        let (Some(archive), Some(entry)) = (self.archive.as_mut(), self.archive_paths.get(path))
        else {
            return false;
        };
        let Ok(file) = archive.by_name(entry) else {
            return false;
        };
        let Ok(bytes) = read_entry(file, MAX_ENTRY_BYTES) else {
            return false;
        };
        self.files.insert(path.to_owned(), bytes.into());
        true
    }
}

fn is_ui_json(path: &str) -> bool {
    path.starts_with("ui/") && path.ends_with(".json")
}

/// Where a texture resolved: the layer and file, loaded or still wanted.
pub enum TextureFile {
    Loaded {
        layer: usize,
        path: String,
        bytes: Arc<[u8]>,
    },
    Wanted {
        layer: usize,
        path: String,
    },
}

#[derive(Default)]
pub struct Workspace {
    layers: Vec<Layer>,
    /// Catalog after layers `0..=i`, with the generation sum it was built from.
    // Initialize empty generations too: copying an unspecified inactive Option
    // payload triggered a macOS allocator classification failure in export tests.
    prefix: Vec<(u64, Option<Arc<Catalog>>)>,
    lang: Option<(u64, Arc<HashMap<String, String>>)>,
    /// Why the bottom layer loaded as an overlay rather than by its index files.
    base_error: Option<String>,
    /// Bumped whenever texture bytes arrive, so paints relook them up.
    texture_generation: u64,
    structure_generation: u64,
}

impl Workspace {
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, index: usize) -> Option<&Layer> {
        self.layers.get(index)
    }

    /// Add an empty layer on top, below the scratch layer if there is one;
    /// returns its index.
    pub fn add_layer(&mut self, name: &str) -> usize {
        let index = self.scratch_index().unwrap_or(self.layers.len());
        self.structure_generation = self.structure_generation.wrapping_add(1);
        self.layers.insert(
            index,
            Layer {
                name: name.to_owned(),
                ..Layer::default()
            },
        );
        self.prefix = vec![(0, None); self.layers.len()];
        self.lang = None;
        index
    }

    pub fn scratch_index(&self) -> Option<usize> {
        self.layers.iter().position(|layer| layer.scratch)
    }

    /// Create `ui/<stem>.json` (the first free `scratch`, `scratch_2`, ...) in
    /// the scratch layer, making that layer on top if needed, and list it in
    /// the layer's `_ui_defs.json`. Empty `text` writes a starter screen.
    /// Returns the scratch layer and the new path.
    pub fn new_scratch_file(&mut self, text: &str) -> (usize, String) {
        let layer = match self.scratch_index() {
            Some(layer) => layer,
            None => {
                self.structure_generation = self.structure_generation.wrapping_add(1);
                self.layers.push(Layer {
                    name: SCRATCH.to_owned(),
                    scratch: true,
                    ..Layer::default()
                });
                self.prefix.push((0, None));
                let layer = self.layers.len() - 1;
                self.edit(layer, GLOBALS, "{}\n");
                layer
            }
        };
        let taken = |stem: &str| {
            self.layers[layer]
                .files
                .contains_key(&format!("ui/{stem}.json"))
        };
        let stem = (1..)
            .map(|n| match n {
                1 => SCRATCH.to_owned(),
                n => format!("{SCRATCH}_{n}"),
            })
            .find(|stem| !taken(stem))
            .expect("an unbounded range has a free name");
        let path = format!("ui/{stem}.json");
        let text = if text.trim().is_empty() {
            SCRATCH_TEMPLATE.replace("{namespace}", &stem)
        } else {
            text.to_owned()
        };
        self.edit(layer, &path, &text);
        let listed: Vec<String> = self.layers[layer]
            .ui_paths()
            .filter(|p| !p.starts_with("ui/_"))
            .map(str::to_owned)
            .collect();
        let defs = serde_json::json!({ "ui_defs": listed });
        self.edit(layer, UI_DEFS, &format!("{defs:#}\n"));
        (layer, path)
    }

    pub fn remove_layer(&mut self, index: usize) {
        if index < self.layers.len() {
            self.layers.remove(index);
            self.structure_generation = self.structure_generation.wrapping_add(1);
            self.prefix = vec![(0, None); self.layers.len()];
            self.lang = None;
        }
    }

    /// Add host files to `layer`, stripping the folder that holds the pack.
    /// `pending` paths exist but arrive later through [`Self::supply`].
    pub fn add_files(&mut self, layer: usize, files: Vec<(String, Vec<u8>)>, pending: Vec<String>) {
        let all: Vec<&str> = files
            .iter()
            .map(|(path, _)| path.as_str())
            .chain(pending.iter().map(String::as_str))
            .collect();
        let root = pack_root(&all);
        let Some(target) = self.layers.get_mut(layer) else {
            return;
        };
        for (path, bytes) in files {
            if let Some(path) = strip_root(&path, &root) {
                target.pending.remove(&path);
                target.files.insert(path, bytes.into());
            }
        }
        for path in pending {
            if let Some(path) = strip_root(&path, &root)
                && !target.files.contains_key(&path)
            {
                target.pending.insert(path);
            }
        }
        target.generation += 1;
    }

    /// Open a zip as `layer`'s contents: ui json and lang load now, the rest on demand.
    pub fn add_archive(&mut self, layer: usize, bytes: Vec<u8>) -> Result<(), String> {
        let bytes: Arc<[u8]> = bytes.into();
        let mut archive =
            zip::ZipArchive::new(Cursor::new(Arc::clone(&bytes))).map_err(|e| e.to_string())?;
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        let root = pack_root(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut paths = BTreeMap::new();
        for name in names.iter().filter(|name| !name.ends_with('/')) {
            if let Some(path) = strip_root(&name.replace('\\', "/"), &root) {
                paths.insert(path, name.clone());
            }
        }
        let mut eager = Vec::new();
        for (path, name) in &paths {
            if is_ui_json(path) || path.starts_with("texts/") {
                let file = archive.by_name(name).map_err(|e| e.to_string())?;
                let contents = read_entry(file, MAX_ENTRY_BYTES).map_err(|e| e.to_string())?;
                eager.push((path.clone(), contents));
            }
        }
        let Some(target) = self.layers.get_mut(layer) else {
            return Err("no such layer".into());
        };
        for (path, contents) in eager {
            target.files.insert(path, contents.into());
        }
        target.archive = Some(archive);
        target.archive_paths = paths;
        target.generation += 1;
        Ok(())
    }

    /// Bytes for a file previously announced as pending.
    pub fn supply(&mut self, layer: usize, path: &str, bytes: Vec<u8>) {
        self.texture_generation += 1;
        if let Some(target) = self.layers.get_mut(layer) {
            target.pending.remove(path);
            target.files.insert(path.to_owned(), bytes.into());
        }
    }

    /// Replace a file with edited text.
    pub fn edit(&mut self, layer: usize, path: &str, text: &str) {
        if let Some(target) = self.layers.get_mut(layer) {
            if !target.edited.contains(path) {
                let before = target.files.get(path).cloned();
                target.originals.insert(path.to_owned(), before);
            }
            target
                .files
                .insert(path.to_owned(), text.as_bytes().to_vec().into());
            target.edited.insert(path.to_owned());
            target.generation += 1;
        }
    }

    pub fn texture_generation(&self) -> u64 {
        self.texture_generation
    }

    pub fn base_error(&self) -> Option<&str> {
        self.base_error.as_deref()
    }

    /// Every layer's generation, which changes whenever its ui or lang text does.
    pub fn generation(&self) -> u64 {
        self.generation_upto(self.layers.len())
    }

    fn generation_upto(&self, count: usize) -> u64 {
        self.layers[..count].iter().enumerate().fold(
            self.structure_generation,
            |sum, (index, layer)| {
                sum.wrapping_mul(1_000_003)
                    .wrapping_add(layer.generation ^ index as u64)
            },
        )
    }

    /// The merged catalog of every layer.
    pub fn catalog(&mut self) -> Arc<Catalog> {
        if self.layers.is_empty() {
            return Arc::new(Catalog::default());
        }
        let top = self.layers.len() - 1;
        self.catalog_upto(top)
    }

    fn catalog_upto(&mut self, index: usize) -> Arc<Catalog> {
        let generation = self.generation_upto(index + 1);
        let (built, cached) = &self.prefix[index];
        if let Some(catalog) = cached
            && *built == generation
        {
            return Arc::clone(catalog);
        }
        let catalog = if index == 0 {
            let (catalog, error) = base_catalog(&self.layers[0]);
            self.base_error = error;
            catalog
        } else {
            let mut catalog = (*self.catalog_upto(index - 1)).clone();
            catalog.apply_pack(self.layers[index].ui_files());
            catalog
        };
        let catalog = Arc::new(catalog);
        self.prefix[index] = (generation, Some(Arc::clone(&catalog)));
        catalog
    }

    /// `texts/en_US.lang` merged bottom to top.
    pub fn lang(&mut self) -> Arc<HashMap<String, String>> {
        let generation = self.generation();
        if let Some((built, lang)) = &self.lang
            && *built == generation
        {
            return Arc::clone(lang);
        }
        let mut table = HashMap::new();
        for layer in &self.layers {
            if let Some(bytes) = layer.file(LANG) {
                parse_lang(&String::from_utf8_lossy(bytes), &mut table);
            }
        }
        let table = Arc::new(table);
        self.lang = Some((generation, Arc::clone(&table)));
        table
    }

    /// The topmost file for texture `key` (spelled with or without extension),
    /// extracting it from an archive when needed.
    pub fn texture_file(&mut self, key: &str) -> Option<TextureFile> {
        let candidates = texture_candidates(key);
        for layer in (0..self.layers.len()).rev() {
            let found = self.find_in(layer, &candidates);
            let Some(path) = found else {
                continue;
            };
            let target = &mut self.layers[layer];
            if !target.files.contains_key(&path) && !target.extract(&path) {
                return Some(TextureFile::Wanted { layer, path });
            }
            let bytes = Arc::clone(&target.files[&path]);
            return Some(TextureFile::Loaded { layer, path, bytes });
        }
        None
    }

    /// The topmost sidecar json for texture `key`, if loaded.
    pub fn sidecar(&mut self, key: &str) -> Option<Arc<[u8]>> {
        let stem = texture_stem(key);
        let candidates = [format!("{stem}.json")];
        for layer in (0..self.layers.len()).rev() {
            if let Some(path) = self.find_in(layer, &candidates) {
                let target = &mut self.layers[layer];
                if target.files.contains_key(&path) || target.extract(&path) {
                    return target.files.get(&path).cloned();
                }
                return None;
            }
        }
        None
    }

    /// The first candidate `layer` holds, exactly or ignoring ASCII case.
    fn find_in(&self, layer: usize, candidates: &[String]) -> Option<String> {
        let target = &self.layers[layer];
        if let Some(path) = candidates.iter().find(|path| target.has(path)) {
            return Some(path.clone());
        }
        let folded: Vec<String> = candidates.iter().map(|c| c.to_ascii_lowercase()).collect();
        target
            .files
            .keys()
            .chain(target.pending.iter())
            .chain(target.archive_paths.keys())
            .find(|path| folded.contains(&path.to_ascii_lowercase()))
            .cloned()
    }

    /// A file's bytes, extracting it from the layer's archive when needed;
    /// `None` when absent or not yet supplied.
    pub fn read_file(&mut self, layer: usize, path: &str) -> Option<Arc<[u8]>> {
        let target = self.layers.get_mut(layer)?;
        if !target.files.contains_key(path) {
            target.extract(path);
        }
        target.files.get(path).cloned()
    }
}

fn base_catalog(layer: &Layer) -> (Catalog, Option<String>) {
    let error = if layer.files.contains_key(UI_DEFS) && layer.files.contains_key(GLOBALS) {
        match Catalog::from_files(layer.ui_files()) {
            Ok(catalog) => return (catalog, None),
            Err(error) => Some(error.to_string()),
        }
    } else {
        None
    };
    let mut catalog = Catalog::default();
    catalog.apply_pack(layer.ui_files());
    (catalog, error)
}

/// The folder prefix that holds the pack: the shallowest `manifest.json`'s
/// directory, else whatever precedes the first `ui/` segment.
fn pack_root(paths: &[&str]) -> String {
    let normalized: Vec<String> = paths.iter().map(|path| path.replace('\\', "/")).collect();
    let manifest = normalized
        .iter()
        .filter_map(|path| path.strip_suffix("manifest.json"))
        .filter(|prefix| prefix.is_empty() || prefix.ends_with('/'))
        .min_by_key(|prefix| prefix.len());
    if let Some(prefix) = manifest {
        return prefix.to_owned();
    }
    normalized
        .iter()
        .filter_map(|path| {
            if path.starts_with("ui/") {
                return Some(String::new());
            }
            path.find("/ui/").map(|index| path[..=index].to_owned())
        })
        .min_by_key(String::len)
        .unwrap_or_default()
}

fn strip_root(path: &str, root: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    path.strip_prefix(root)
        .filter(|rest| !rest.is_empty())
        .map(str::to_owned)
}

fn texture_stem(key: &str) -> &str {
    for extension in [".png", ".jpg", ".jpeg", ".tga"] {
        if let Some(stem) = key.strip_suffix(extension) {
            return stem;
        }
    }
    key
}

fn texture_candidates(key: &str) -> Vec<String> {
    let key = key.trim_start_matches('/');
    let stem = texture_stem(key);
    let mut candidates = Vec::with_capacity(5);
    if stem != key {
        candidates.push(key.to_owned());
    }
    for extension in ["png", "tga", "jpg", "jpeg"] {
        candidates.push(format!("{stem}.{extension}"));
    }
    candidates
}

/// `key=value` lines; `##` lines and `\t#` trailers are comments.
fn parse_lang(text: &str, table: &mut HashMap<String, String>) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("##") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.split_once("\t#").map_or(value, |(value, _)| value);
        let key = key.trim();
        if !key.is_empty() {
            table.insert(key.to_owned(), value.to_owned());
        }
    }
}

#[cfg(test)]
mod cache_tests;

#[cfg(test)]
mod tests {
    use super::*;

    // A pack dropped inside a wrapper folder still lands at its own root.
    #[test]
    fn wrapper_folders_are_stripped_to_the_pack_root() {
        let mut workspace = Workspace::default();
        let layer = workspace.add_layer("pack");
        workspace.add_files(
            layer,
            vec![
                ("Pack/manifest.json".into(), b"{}".to_vec()),
                ("Pack/ui/a.json".into(), b"{}".to_vec()),
            ],
            vec!["Pack/textures/ui/x.png".into()],
        );
        assert!(workspace.layers[0].file("ui/a.json").is_some());
        assert!(matches!(
            workspace.texture_file("textures/ui/x"),
            Some(TextureFile::Wanted { path, .. }) if path == "textures/ui/x.png"
        ));
    }

    #[test]
    fn lang_comments_are_dropped() {
        let mut table = HashMap::new();
        parse_lang("## header\na.b=Hello\t#note\nc=d=e\n", &mut table);
        assert_eq!(table["a.b"], "Hello");
        assert_eq!(table["c"], "d=e");
    }
}

/// Reads one archived file under the editor's byte policy.
fn read_entry(reader: impl Read, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "archive entry exceeds byte limit",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn review_archive_entries_are_rejected_instead_of_truncated() {
        assert_eq!(read_entry(&b"12345678"[..], 8).unwrap(), b"12345678");
        assert!(read_entry(&b"123456789"[..], 8).is_err());
    }

    #[test]
    fn review_replacing_a_layer_changes_its_cache_generation() {
        let mut workspace = Workspace::default();
        let first = workspace.add_layer("first");
        workspace.add_files(first, vec![("ui/a.json".into(), b"{}".to_vec())], vec![]);
        let before = workspace.generation();
        workspace.remove_layer(first);
        let second = workspace.add_layer("second");
        workspace.add_files(
            second,
            vec![("ui/a.json".into(), b"{\"namespace\":\"new\"}".to_vec())],
            vec![],
        );
        assert_ne!(workspace.generation(), before);
    }
}
