use super::hud_renderers;
use json_ui::Catalog;

mod chat;

#[cfg(test)]
mod tests;

/// Builds the same layered catalog for bootstrap and live reload workers.
pub(in crate::ui_runtime::presentation::forms) fn layer_pack_catalog(
    base: &Catalog,
    layers: &[Vec<(String, Vec<u8>)>],
) -> Catalog {
    let builtin = hud_renderers::with_java_hud(base);
    let mut catalog = builtin.clone();
    for files in layers {
        catalog.apply_pack(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        );
    }
    chat::prefer_builtin(&mut catalog, &builtin);
    for note in catalog.diagnostics().iter().skip(base.diagnostics().len()) {
        bevy::log::debug!(note, "server ui pack");
    }
    catalog
}
