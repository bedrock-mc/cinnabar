//! Counted terrain failures for one compiled stack, without retaining pack content.

use std::{cell::RefCell, collections::BTreeMap};

use resource_pack::LayeredPackView;

use pack_runtime::pack_textures::parse_pack_json;

/// Distinct routes kept as evidence for each failure reason.
const MAX_ROUTES_PER_REASON: usize = 8;
const MAX_LABEL_CHARS: usize = 160;

/// Why a requested terrain route produced no texture.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum FailureReason {
    /// A pack's terrain catalog is malformed.
    InvalidCatalog,
    /// The merged catalog holds no path for the requested key.
    TerrainKeyMissing,
    /// No pack in the stack carries the named raster.
    TextureFileMissing,
    /// The raster is present but could not be decoded.
    ImageDecodeFailed,
    /// The route names a texture set rather than a raster.
    TextureSetUnresolved,
    /// The overlay's texture array has no free layer left.
    TextureArrayFull,
    /// A flipbook selects a terrain tile or atlas index that does not exist.
    AnimationDestinationMissing,
    /// Full-resolution pixels cannot fit the retained source or upload budget.
    TextureBudgetExceeded,
}

impl FailureReason {
    /// The stable name reported in the aggregate log line.
    fn key(self) -> &'static str {
        match self {
            Self::InvalidCatalog => "invalid_catalog",
            Self::TerrainKeyMissing => "terrain_key_missing",
            Self::TextureFileMissing => "texture_file_missing",
            Self::ImageDecodeFailed => "image_decode_failed",
            Self::TextureSetUnresolved => "texture_set_unresolved",
            Self::TextureArrayFull => "texture_array_full",
            Self::AnimationDestinationMissing => "animation_destination_missing",
            Self::TextureBudgetExceeded => "texture_budget_exceeded",
        }
    }
}

/// Total rejections plus the bounded routes exemplifying them.
#[derive(Clone, Default)]
struct ReasonFailures {
    count: usize,
    routes: Vec<(String, String)>,
}

/// Counts every rejected terrain route and names a bounded sample of each, so a
/// stack with thousands of failures still says which stage rejected them.
#[derive(Clone, Default)]
pub(super) struct TextureDiagnostics {
    reasons: RefCell<BTreeMap<FailureReason, ReasonFailures>>,
}

impl TextureDiagnostics {
    /// Checks every pack's terrain catalog, recording malformed ones.
    pub(super) fn catalogs(&self, view: &LayeredPackView) {
        for bytes in view.read_layers("textures/terrain_texture.json") {
            if !parse_pack_json(&bytes).is_some_and(|root| root["texture_data"].is_object()) {
                self.failure(
                    FailureReason::InvalidCatalog,
                    "",
                    Some("textures/terrain_texture.json"),
                );
            }
        }
    }

    /// Records one rejected route under its reason.
    pub(super) fn failure(&self, reason: FailureReason, key: &str, path: Option<&str>) {
        let mut reasons = self.reasons.borrow_mut();
        let entry = reasons.entry(reason).or_default();
        entry.count += 1;
        if entry.routes.len() < MAX_ROUTES_PER_REASON {
            entry
                .routes
                .push((label(key), label(path.unwrap_or("<none>"))));
        }
    }

    /// Emits one aggregate line per observed reason.
    pub(super) fn emit(&self) {
        for (reason, failures) in self.reasons.borrow().iter() {
            bevy::log::warn!(
                reason = reason.key(),
                count = failures.count,
                reported_routes = failures.routes.len(),
                routes = %routes(&failures.routes),
                "terrain texture failures"
            );
        }
    }
}

/// Bounds labels on character boundaries even when server names contain Unicode.
fn label(value: &str) -> String {
    value.chars().take(MAX_LABEL_CHARS).collect()
}

/// Renders the retained routes, with the count still unrepresented.
fn routes(routes: &[(String, String)]) -> String {
    routes
        .iter()
        .map(|(key, path)| format!("{key} -> {path}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Distinguishes absent rasters from present inputs that failed bounded decoding.
pub(super) fn failure_reason(view: &LayeredPackView, path: &str) -> FailureReason {
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
        FailureReason::ImageDecodeFailed
    } else if exists(&format!("{path}.texture_set.json")) {
        FailureReason::TextureSetUnresolved
    } else {
        FailureReason::TextureFileMissing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reason_aggregates_its_own_routes() {
        let diagnostics = TextureDiagnostics::default();
        for index in 0..24 {
            diagnostics.failure(
                FailureReason::TextureFileMissing,
                &format!("pack:missing_{index}"),
                Some("textures/blocks/missing"),
            );
        }
        for _ in 0..3 {
            diagnostics.failure(FailureReason::ImageDecodeFailed, "pack:broken", Some("a/b"));
        }
        let reasons = diagnostics.reasons.borrow();
        let missing = &reasons[&FailureReason::TextureFileMissing];
        assert_eq!(missing.count, 24);
        assert_eq!(missing.routes.len(), MAX_ROUTES_PER_REASON);
        assert_eq!(missing.routes[0].0, "pack:missing_0");
        assert_eq!(reasons[&FailureReason::ImageDecodeFailed].count, 3);
    }

    #[test]
    fn reasons_keep_their_stable_names_apart() {
        let names = [
            FailureReason::InvalidCatalog,
            FailureReason::TerrainKeyMissing,
            FailureReason::TextureFileMissing,
            FailureReason::ImageDecodeFailed,
            FailureReason::TextureSetUnresolved,
            FailureReason::TextureArrayFull,
            FailureReason::TextureBudgetExceeded,
        ]
        .map(FailureReason::key);
        let mut unique = names.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn labels_bound_unicode_without_panicking() {
        let text = "界".repeat(MAX_LABEL_CHARS + 1);
        assert_eq!(label(&text).chars().count(), MAX_LABEL_CHARS);
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
        let archive = bedrock_packs::ResourcePackArchive::unencrypted(
            id.parse().unwrap(),
            "1.0.0".into(),
            String::new(),
            zip.finish().unwrap().into_inner(),
        );
        LayeredPackView::new(resource_pack::validate_handoff(
            bedrock_packs::ResourcePackHandoff::from_archives(vec![archive]),
        ))
    }

    #[test]
    fn failure_reasons_distinguish_missing_rasters_and_invalid_images() {
        let view = fixture();
        assert_eq!(
            failure_reason(&view, "textures/missing"),
            FailureReason::TextureFileMissing
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/gen"),
            FailureReason::ImageDecodeFailed
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/gen.png"),
            FailureReason::ImageDecodeFailed
        );
        assert_eq!(
            failure_reason(&view, "textures/blocks/set"),
            FailureReason::TextureSetUnresolved
        );
    }
}
