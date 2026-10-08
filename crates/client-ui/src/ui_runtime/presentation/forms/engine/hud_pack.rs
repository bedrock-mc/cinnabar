//! Installs client settings and the built-in HUD pack.

#[cfg(test)]
mod crosshair_tests;

/// Layers the built-in HUD and menu styling over vanilla before server packs.
pub(super) fn with_java_hud(vanilla: &json_ui::Catalog) -> json_ui::Catalog {
    let mut catalog = vanilla.clone();
    let hud_files = ui::native_hud::JAVA_HUD_PACK
        .iter()
        .map(|(path, _, bytes)| (*path, *bytes));
    super::super::graphics_expander::install(&mut catalog);
    super::super::always_sprint_setting::install(&mut catalog);
    super::super::vsync_setting::install(&mut catalog);
    super::super::java_animations_setting::install(&mut catalog);
    super::super::discord_presence_setting::install(&mut catalog);
    super::super::crosshair_settings::install(&mut catalog);
    catalog.apply_pack(hud_files);
    catalog.apply_pack(
        [(
            "ui/ui_art_assets_common.json",
            super::menu_renderers::TITLE_PANEL_OVERLAY,
        )]
        .into_iter()
        .chain(super::menu_renderers::NO_COPYRIGHT_OVERLAYS),
    );
    super::super::loading_screen::install_brand_layout(&mut catalog);
    super::super::enhanced_setting::install(&mut catalog);
    super::super::chat_position::install(&mut catalog);
    catalog
}
