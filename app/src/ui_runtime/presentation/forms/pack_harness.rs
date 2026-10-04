//! Startup fixture that exercises the app's carrier-loading entry points.

pub(crate) use client_ui::test_support::pack_harness::{
    action_form, carrier, drawn_texts, dump, engine_presentation, env_glyphs, env_pack, font,
    menu_nodes, menu_runtime, menu_translation, scratch_dir,
};
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use std::path::Path;

/// Resolves a fixture asset through this worktree's installed assets symlink.
fn local(path: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.local")
        .join(path)
}

/// Loads the startup texture layout, including HUD, icons, models and optional OreUI art.
pub fn startup_presentation() -> Option<UiPresentationRuntime> {
    let world = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate::asset_startup::DEFAULT_ASSET_PATH);
    for path in [
        crate::asset_startup::hud_asset_path(&world),
        crate::asset_startup::icon_asset_path(&world),
        crate::asset_startup::entity_asset_path(&world),
    ] {
        if !path.exists() {
            eprintln!(
                "skipping startup presentation fixture: missing {}; make assets",
                path.display()
            );
            return None;
        }
    }
    let hud = crate::asset_startup::require_hud_assets(&world)
        .expect("load installed HUD fixture")
        .into_runtime();
    let icons = crate::asset_startup::require_icon_assets(
        &world,
        include_str!("../../../../../assets/vanilla-source.json"),
    )
    .expect("load installed icon fixture")
    .into_runtime();
    let entities = assets::RuntimeEntityAssets::decode(
        &std::fs::read(crate::asset_startup::entity_asset_path(&world))
            .expect("read installed entity fixture"),
    )
    .expect("decode installed entity fixture");
    let mut presentation = UiPresentationRuntime::with_hud_and_icons(font(), hud, icons).unwrap();
    presentation.enable_json_ui(carrier()?).unwrap();
    presentation.set_form_texture_fallbacks(
        &entities,
        local(&crate::install_layout::vanilla_pack_relative()),
    );
    if let Some(images) = client_ui::ui_runtime::oreui_assets::load_optional_oreui_images() {
        presentation.enable_oreui_originals(images).unwrap();
    }
    presentation
        .set_gui_models(&assets::RuntimeAssets::diagnostic(), &entities)
        .unwrap();
    Some(presentation)
}
