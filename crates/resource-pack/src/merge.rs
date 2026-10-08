//! Merged reads of pack catalogs across the layered stack, for consumers that
//! need one view. `winning_files("ui/", "json")` feeds `json_ui::Catalog::overlay_text`;
//! `merged_sound_definitions` is the audio hook (no runtime audio consumer yet).

use serde_json::{Map, Value};

use crate::{LayeredPackView, normalize_jsonc};

pub const MAX_MERGED_ENTRIES: usize = 65_536;
pub const MAX_WINNING_FILES: usize = 1024;
pub const MAX_WINNING_BYTES: usize = 32 * 1024 * 1024;

fn parse(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(&normalize_jsonc(bytes)?).ok()
}

/// Folds `object` into `merged`; a later (higher) layer replaces a key wholesale.
fn fold(merged: &mut Map<String, Value>, object: Map<String, Value>) {
    for (key, value) in object {
        if key == "format_version" {
            continue;
        }
        if merged.len() >= MAX_MERGED_ENTRIES && !merged.contains_key(&key) {
            continue;
        }
        merged.insert(key, value);
    }
}

impl LayeredPackView {
    /// Merges the object under `section` (the root when `None`) of every layer's
    /// copy of `path`. A layer that is unreadable or lacks the section is skipped.
    #[must_use]
    pub fn merged_json_object(&self, path: &str, section: Option<&str>) -> Map<String, Value> {
        let mut merged = Map::new();
        for layer in self.read_layers(path) {
            let Some(mut root) = parse(&layer) else {
                continue;
            };
            let object = match section {
                Some(name) => root.get_mut(name).map(Value::take),
                None => Some(root),
            };
            if let Some(Value::Object(object)) = object {
                fold(&mut merged, object);
            }
        }
        merged
    }

    /// Merges `sounds/sound_definitions.json` across layers, accepting both the
    /// sectioned format and the legacy flat one; a higher layer replaces a name.
    #[must_use]
    pub fn merged_sound_definitions(&self) -> Map<String, Value> {
        let mut merged = Map::new();
        for layer in self.read_layers("sounds/sound_definitions.json") {
            let Some(Value::Object(mut root)) = parse(&layer) else {
                continue;
            };
            match root.remove("sound_definitions") {
                Some(Value::Object(definitions)) => fold(&mut merged, definitions),
                _ => fold(&mut merged, root),
            }
        }
        merged
    }

    /// Winning copy of every file under `prefix` with `extension`, in path order,
    /// bounded by file count and total bytes; unreadable files are skipped.
    #[must_use]
    pub fn winning_files(&self, prefix: &str, extension: &str) -> Vec<(String, Box<[u8]>)> {
        let mut files = Vec::new();
        let mut total = 0usize;
        for path in self.list(prefix) {
            let matches = path
                .rsplit_once('.')
                .is_some_and(|(_, found)| found.eq_ignore_ascii_case(extension));
            if !matches {
                continue;
            }
            let Some(bytes) = self.read(path) else {
                continue;
            };
            total = total.saturating_add(bytes.len());
            if files.len() >= MAX_WINNING_FILES || total > MAX_WINNING_BYTES {
                break;
            }
            files.push((path.to_owned(), bytes));
        }
        files
    }
}

#[cfg(all(test, feature = "handoff"))]
mod tests {
    use std::io::{Cursor, Write};

    use protocol::{ResourcePackArchive, ResourcePackHandoff};
    use uuid::Uuid;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use crate::{LayeredPackView, validate_handoff};

    fn pack(id: u128, files: &[(&str, &str)]) -> ResourcePackArchive {
        let id = Uuid::from_u128(id);
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
        );
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, text) in
            std::iter::once(("manifest.json", manifest.as_str())).chain(files.iter().copied())
        {
            writer
                .start_file(path, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        ResourcePackArchive::unencrypted(
            id,
            "1.0.0".into(),
            String::new(),
            writer.finish().unwrap().into_inner(),
        )
    }

    fn view(top: &[(&str, &str)], bottom: &[(&str, &str)]) -> LayeredPackView {
        LayeredPackView::new(validate_handoff(ResourcePackHandoff::from_archives(vec![
            pack(2, bottom),
            pack(1, top),
        ])))
    }

    // Mixed sectioned and legacy files merge by name with the higher layer winning.
    #[test]
    fn sound_definitions_merge_across_formats() {
        let view = view(
            &[(
                "sounds/sound_definitions.json",
                r#"{"format_version":"1.14.0","sound_definitions":{"a":{"v":"top"},"c":{"v":"c"}}}"#,
            )],
            &[(
                "sounds/sound_definitions.json",
                r#"{"a":{"v":"bottom"},"b":{"v":"b"}}"#,
            )],
        );
        let merged = view.merged_sound_definitions();
        assert_eq!(merged["a"]["v"], "top");
        assert_eq!(merged["b"]["v"], "b");
        assert_eq!(merged["c"]["v"], "c");
        assert!(!merged.contains_key("format_version"));
    }

    #[test]
    fn section_merge_and_winning_files() {
        let view = view(
            &[
                (
                    "blocks.json",
                    r#"{"format_version":[1,1,0],"x":{"sound":"top"}}"#,
                ),
                ("ui/a.json", "{\"t\":1}"),
            ],
            &[
                ("blocks.json", r#"{"x":{"sound":"low"},"y":{}}"#),
                ("ui/a.json", "{\"b\":1}"),
                ("ui/b.json", "{}"),
                ("ui/c.txt", ""),
            ],
        );
        let blocks = view.merged_json_object("blocks.json", None);
        assert_eq!(blocks["x"]["sound"], "top");
        assert!(blocks.contains_key("y"));
        let files = view.winning_files("ui/", "json");
        let names: Vec<_> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["ui/a.json", "ui/b.json"]);
        assert_eq!(&*files[0].1, b"{\"t\":1}");
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_full_maps_still_accept_higher_layer_replacements() {
        let mut merged: Map<String, Value> = (0..MAX_MERGED_ENTRIES - 1)
            .map(|i| (format!("key{i}"), Value::Null))
            .collect();
        merged.insert("z".into(), Value::from("old"));
        let object = serde_json::from_str(r#"{"a_new":0,"z":"new"}"#).unwrap();
        fold(&mut merged, object);
        assert_eq!(merged["z"], "new");
        assert_eq!(merged.len(), MAX_MERGED_ENTRIES);
    }
}
