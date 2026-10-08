//! The first terrain failures of each compiled stack, without retaining pack content.

use std::cell::Cell;

use resource_pack::LayeredPackView;

use super::super::super::resource_packs::parse_pack_json;

const MAX_FAILURES: usize = 8;
const MAX_LABEL_CHARS: usize = 160;

#[derive(Default)]
pub(super) struct TextureDiagnostics {
    emitted: Cell<usize>,
}

impl TextureDiagnostics {
    /// Reports malformed terrain catalogs once while preparing this stack.
    pub(super) fn catalogs(&self, view: &LayeredPackView) {
        for bytes in view.read_layers("textures/terrain_texture.json") {
            if !parse_pack_json(&bytes).is_some_and(|root| root["texture_data"].is_object()) {
                self.failure("", Some("textures/terrain_texture.json"), "invalid_catalog");
            }
        }
    }

    /// Names a failed requested route within the stack's shared diagnostic budget.
    pub(super) fn failure(&self, key: &str, path: Option<&str>, reason: &str) {
        if !self.claim() {
            return;
        }
        bevy::log::warn!(
            texture_key = %label(key),
            texture_path = %label(path.unwrap_or("<none>")),
            reason,
            "PACK_TERRAIN_FAILURE"
        );
    }

    /// Reserves one line and stops counting once the bounded budget is exhausted.
    fn claim(&self) -> bool {
        let emitted = self.emitted.get();
        if emitted >= MAX_FAILURES {
            return false;
        }
        self.emitted.set(emitted + 1);
        true
    }
}

/// Bounds labels on character boundaries even when server names contain Unicode.
fn label(value: &str) -> String {
    value.chars().take(MAX_LABEL_CHARS).collect()
}

/// Distinguishes absent rasters from present inputs that failed bounded decoding.
pub(super) fn failure_reason(view: &LayeredPackView, path: &str) -> &'static str {
    let exists = |candidate: &str| {
        view.stack()
            .packs()
            .iter()
            .any(|pack| pack.contains(candidate))
    };
    if exists(path)
        || ["png", "tga", "jpg", "jpeg"]
            .iter()
            .any(|extension| exists(&format!("{path}.{extension}")))
    {
        "image_decode_failed"
    } else if exists(&format!("{path}.texture_set.json")) {
        "texture_set_unresolved"
    } else {
        "texture_file_missing"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_failure_budget_stays_bounded() {
        let diagnostics = TextureDiagnostics::default();
        for _ in 0..MAX_FAILURES {
            assert!(diagnostics.claim());
        }
        for _ in 0..1000 {
            assert!(!diagnostics.claim());
        }
        assert_eq!(diagnostics.emitted.get(), MAX_FAILURES);
        assert!(TextureDiagnostics::default().claim());
    }

    #[test]
    fn terrain_failure_labels_bound_unicode_without_panicking() {
        let text = "界".repeat(MAX_LABEL_CHARS + 1);
        assert_eq!(label(&text).chars().count(), MAX_LABEL_CHARS);
    }

    #[test]
    fn terrain_failure_reasons_distinguish_missing_rasters_and_invalid_images() {
        let view = fixture();
        assert_eq!(
            failure_reason(&view, "textures/missing"),
            "texture_file_missing"
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/gen"),
            "image_decode_failed"
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/gen.png"),
            "image_decode_failed"
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/set"),
            "texture_set_unresolved"
        );
    }

    /// Admits a fixture containing an invalid image and an unresolved texture set.
    fn fixture() -> LayeredPackView {
        use std::io::Write;
        let id = "00000000-0000-0000-0000-000000000001";
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in [
            ("manifest.json", manifest.as_bytes()),
            ("textures/blocks/gen.png", b"invalid".as_slice()),
            ("textures/blocks/set.texture_set.json", b"{}".as_slice()),
        ] {
            zip.start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        let archive = protocol::ResourcePackArchive::unencrypted(
            id.parse().unwrap(),
            "1.0.0".into(),
            String::new(),
            zip.finish().unwrap().into_inner(),
        );
        LayeredPackView::new(resource_pack::validate_handoff(
            protocol::ResourcePackHandoff::from_archives(vec![archive]),
        ))
    }
}
