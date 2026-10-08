//! Pack UI definitions follow the index's paths, including custom extensions.

use std::{collections::BTreeSet, sync::Arc};

use resource_pack::LayeredPackView;

use super::ServerUiPack;

/// Bound on definition bytes handed to the form engine across the whole stack.
const MAX_SERVER_UI_BYTES: usize = 16 * 1024 * 1024;
const INDEX: &str = "ui/_ui_defs.json";

/// Collects indexed definitions and records dependencies for lazily loaded artwork.
pub(super) fn collect_server_ui(view: &LayeredPackView) -> Option<Arc<ServerUiPack>> {
    let mut total = 0usize;
    let mut declared = BTreeSet::new();
    let mut pack = ServerUiPack::default();
    for layer in view.layers() {
        let remaining = MAX_SERVER_UI_BYTES.saturating_sub(total) as u64;
        if let Some(index) = layer.read_file_with_limit(INDEX, remaining).ok().flatten()
            && let Ok(paths) = json_ui::Catalog::declared_paths(&index)
        {
            declared.extend(paths);
        }
        let paths: BTreeSet<_> = layer
            .files_under("ui/")
            .iter()
            .filter(|path| path.ends_with(".json"))
            .map(|path| (*path).to_owned())
            .chain(declared.iter().cloned())
            .collect();
        let mut files = Vec::new();
        for path in paths {
            let remaining = MAX_SERVER_UI_BYTES.saturating_sub(total) as u64;
            let Some(bytes) = layer.read_file_with_limit(&path, remaining).ok().flatten() else {
                continue;
            };
            total += bytes.len();
            files.push((path, bytes.into_vec()));
        }
        pack.ui_layers.push(files);
    }
    // Art may live anywhere in the admitted archive. Its image and sidecar
    // bytes are consumed later by the renderer, so record them for reloads.
    view.track_contents("textures/");
    let mut has_images = false;
    for path in view.list_with_suffixes("", ServerUiPack::image_extensions()) {
        if ServerUiPack::is_image_path(path) {
            has_images = true;
            view.track_contents(path);
            if let Some((stem, _)) = path.rsplit_once('.') {
                view.track_contents(&format!("{stem}.json"));
            }
        }
    }
    if pack.is_empty() && !has_images && view.list("textures/").is_empty() {
        return None;
    }
    pack.view = Some(view.clone());
    Some(Arc::new(pack))
}
