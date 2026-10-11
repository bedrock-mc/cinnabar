//! Pack language preparation shared by initial admission and reloads.

use resource_pack::LayeredPackView;
use std::sync::Arc;

/// The selected UI language; unset means the base English table only.
static ACTIVE_LANG_PATH: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Selects the overlay locale without publishing any prepared presentation.
pub fn set_active_language(code: &str) {
    *ACTIVE_LANG_PATH
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        (code != "en_US").then(|| format!("texts/{code}.lang"));
}

/// Returns the same locale used by the language overlay and its font resources.
pub fn active_language_code() -> String {
    ACTIVE_LANG_PATH
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_deref()
        .and_then(|path| path.strip_prefix("texts/")?.strip_suffix(".lang"))
        .unwrap_or("en_US")
        .to_owned()
}

const SERVER_LANG_PATH: &str = "texts/en_US.lang";

/// Merges every pack's language file so a higher-precedence pack overrides a
/// key and keys it does not define still come from lower packs; the UI
/// language's files override en_US. Lowest layers are dropped first if the
/// merged text would exceed the overlay input bound.
pub fn merged_server_lang(view: &LayeredPackView) -> Option<Arc<assets::ServerLangOverlay>> {
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut layers = view.read_layers(SERVER_LANG_PATH);
    let active = ACTIVE_LANG_PATH
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(active) = active.as_deref() {
        layers.extend(view.read_layers(active));
    }
    for layer in layers.into_iter().rev() {
        let text = layer
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&layer)
            .to_vec();
        let Some(next) = total.checked_add(text.len() + 1) else {
            break;
        };
        if next > assets::MAX_SERVER_LANG_INPUT_BYTES {
            break;
        }
        total = next;
        kept.push(text);
    }
    if kept.is_empty() {
        return None;
    }
    // The overlay keeps the last definition of a key, so write lowest first.
    let mut merged = Vec::with_capacity(total);
    for text in kept.iter().rev() {
        merged.extend_from_slice(text);
        merged.push(b'\n');
    }
    assets::ServerLangOverlay::read(merged.len(), |output| {
        output.copy_from_slice(&merged);
        true
    })
}
