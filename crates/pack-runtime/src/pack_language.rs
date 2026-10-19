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

/// Merges every readable English and selected-locale source in precedence order.
/// A higher layer overrides matching keys while lower layers retain their other keys.
pub fn merged_server_lang(view: &LayeredPackView) -> Option<Arc<assets::ServerLangOverlay>> {
    let active = ACTIVE_LANG_PATH
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let sources = [Some(SERVER_LANG_PATH), active.as_deref()]
        .into_iter()
        .flatten()
        .flat_map(|path| {
            view.layers().enumerate().filter_map(move |(layer_index, layer)| {
                match layer.read_file(path) {
                    Ok(source) => source,
                    Err(error) => {
                        tracing::warn!(path, layer_index, reason = %error, "skipping unreadable server language file");
                        None
                    }
                }
            })
        });
    assets::ServerLangOverlay::from_layers(sources)
}

#[cfg(test)]
mod tests;
