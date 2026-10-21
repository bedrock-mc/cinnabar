//! Every screen family's registration with the engine, in the order its catalog patches layer.

use std::sync::LazyLock;

use ui_engine::engine::screen::{Screen, ScreenRegistry};

static REGISTRY: LazyLock<ScreenRegistry> = LazyLock::new(compose);

/// The registry the engine and the server-pack workers share.
pub(in crate::ui_runtime::presentation) fn registry() -> &'static ScreenRegistry {
    &REGISTRY
}

fn compose() -> ScreenRegistry {
    let mut registry = ScreenRegistry::new(ui_screens::menu_screens::retail_context());
    for &reference in json_ui::ENGINE_SCREENS {
        registry.screen(reference);
    }
    ui_screens::global_resources::register(&mut registry);
    ui_screens::credits_screen::CreditsScreen::register(&mut registry);
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(ui_screens::graphics_expander::install);
    catalog.layer_builtin(ui_screens::always_sprint_setting::install);
    catalog.layer_builtin(ui_screens::vsync_setting::install);
    catalog.layer_builtin(ui_screens::vrr_setting::install);
    catalog.layer_builtin(ui_screens::motion_blur_setting::install);
    catalog.layer_builtin(ui_screens::smaa_setting::install);
    catalog.layer_builtin(ui_screens::view_bobbing_setting::install);
    catalog.layer_builtin(ui_screens::java_animations_setting::install);
    catalog.layer_builtin(ui_screens::skybox_setting::install);
    catalog.layer_builtin(ui_screens::discord_presence_setting::install);
    catalog.layer_builtin(ui_screens::crosshair_settings::install);
    ui_screens::hud::register(&mut registry);
    let catalog = registry.catalog_mut();
    catalog.layer_builtin(ui_screens::menu_renderers::install_brand_overlays);
    catalog.layer_builtin(ui_screens::menu_policy::apply);
    catalog.after_layers(|catalog, _| {
        ui_screens::menu_policy::apply(catalog);
        Vec::new()
    });
    catalog.layer_builtin(ui_screens::loading_screen::install_brand_layout);
    catalog.layer_builtin(ui_screens::disconnect_retry::install);
    catalog.layer_builtin(ui_screens::enhanced_setting::install);
    catalog.layer_builtin(ui_screens::chat_position::install);
    ui_screens::chat_screen::register(&mut registry);
    ui_screens::death_screen::register(&mut registry);
    oreui::OreUiScreen::register(&mut registry);
    registry
}
