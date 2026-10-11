use super::hud_renderers;
use json_ui::Catalog;

mod chat;
mod titles;

#[cfg(test)]
mod tests;

/// A server pack stack over the built-in catalog, and the catalog the chat screen resolves against.
pub(in crate::ui_runtime::presentation::forms) struct LayeredCatalogs {
    pub(in crate::ui_runtime::presentation::forms) pack: Catalog,
    /// Built-in chat carrying the pack's message filters; `None` when the pack adds none.
    pub(in crate::ui_runtime::presentation::forms) chat: Option<Catalog>,
}

/// The layered pack catalog alone, for tests that do not open chat.
#[cfg(test)]
pub(in crate::ui_runtime::presentation::forms) fn layer_pack_catalog(
    base: &Catalog,
    layers: &[Vec<(String, Vec<u8>)>],
) -> Catalog {
    layer_pack_catalogs(base, layers).pack
}

/// Builds the same layered catalogs for bootstrap and live reload workers.
pub(in crate::ui_runtime::presentation::forms) fn layer_pack_catalogs(
    base: &Catalog,
    layers: &[Vec<(String, Vec<u8>)>],
) -> LayeredCatalogs {
    let builtin = hud_renderers::with_java_hud(base);
    let mut catalog = builtin.clone();
    let mut native_titles = false;
    for files in layers {
        if !native_titles && titles::authored(&catalog, files) {
            titles::restore(&mut catalog, base);
            native_titles = true;
        }
        catalog.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
    }
    let filtered = chat::prefer_builtin(&mut catalog, &builtin);
    for note in catalog.diagnostics().iter().skip(base.diagnostics().len()) {
        bevy::log::debug!(note, "server ui pack");
    }
    // Chat keeps the built-in templates it inherits, so pack overrides of shared ones cannot reach it.
    let chat = (!filtered.is_empty()).then(|| {
        let mut chat = builtin;
        for control in filtered {
            chat.insert(control);
        }
        chat
    });
    LayeredCatalogs {
        pack: catalog,
        chat,
    }
}
