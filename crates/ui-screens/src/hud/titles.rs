use json_ui::Catalog;

const TITLE: &str = "hud_title_text";

pub(super) fn authored(catalog: &Catalog, files: &[(String, Vec<u8>)]) -> bool {
    catalog
        .overlay_controls(
            files
                .iter()
                .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
        )
        .iter()
        .any(|control| control.namespace == "hud" && control.name.split('/').next() == Some(TITLE))
}

/// Server title patches inherit vanilla sizing rather than the built-in HUD's scale.
pub(super) fn restore(catalog: &mut Catalog, base: &Catalog) {
    if let Some(title) = base.lookup("hud", TITLE) {
        catalog.insert(title.clone());
    }
}
