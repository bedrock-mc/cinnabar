//! Every screen family's registration with the engine, in the order its catalog patches layer.

use std::sync::LazyLock;

use ui_engine::engine::screen::{Screen, ScreenRegistry};

static REGISTRY: LazyLock<ScreenRegistry> = LazyLock::new(compose);

/// The registry the engine and the server-pack workers share.
pub(in crate::ui_runtime::presentation) fn registry() -> &'static ScreenRegistry {
    &REGISTRY
}

fn compose() -> ScreenRegistry {
    let mut registry = ScreenRegistry::new(super::menu_screens::retail_context());
    for &reference in json_ui::ENGINE_SCREENS {
        registry.screen(reference);
    }
    super::global_resources::register(&mut registry);
    super::credits_screen::CreditsScreen::register(&mut registry);
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(super::graphics_expander::install);
    catalog.layer_builtin(super::always_sprint_setting::install);
    catalog.layer_builtin(super::vsync_setting::install);
    catalog.layer_builtin(super::vrr_setting::install);
    catalog.layer_builtin(super::motion_blur_setting::install);
    catalog.layer_builtin(super::smaa_setting::install);
    catalog.layer_builtin(super::java_animations_setting::install);
    catalog.layer_builtin(super::skybox_setting::install);
    catalog.layer_builtin(super::discord_presence_setting::install);
    catalog.layer_builtin(super::crosshair_settings::install);
    ui_screens::hud::register(&mut registry);
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(super::menu_renderers::install_brand_overlays);
    catalog.layer_builtin(super::loading_screen::install_brand_layout);
    catalog.layer_builtin(super::disconnect_retry::install);
    catalog.layer_builtin(super::enhanced_setting::install);
    catalog.layer_builtin(super::chat_position::install);
    super::chat_screen::register(&mut registry);
    super::death_screen::register(&mut registry);
    oreui::OreUiScreen::register(&mut registry);
    registry
}
