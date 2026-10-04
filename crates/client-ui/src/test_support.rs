//! Explicit cross-crate UI fixtures, enabled only for development dependencies.

use crate::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use assets::{
    FontTexturePage, GlyphMetrics, HudTexture, HudTextureRole, RuntimeFontCatalog,
    RuntimeHudCatalog, RuntimeUiAssets, UiAtlasPage, UiFile, encode_font_catalog,
    encode_hud_catalog, encode_ui_catalog,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

pub use crate::ui_runtime::presentation::forms::pack_harness;

/// Models the shipped atlas: ink 16 texels tall sitting 14 above the baseline
/// with a 12-texel advance, so layout geometry in these tests matches what the
/// client actually lays out. A fixture whose glyphs hang below the baseline
/// instead makes every row report far more height than it occupies.
pub fn fixture_font() -> Arc<RuntimeFontCatalog> {
    let pixels = vec![255; 16 * 16 * 4].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/page.png".into(),
        source_bytes: pixels.len() as u32,
        source_sha256: [1; 32],
        pixels_sha256: Sha256::digest(&pixels).into(),
        width: 16,
        height: 16,
        rgba8: pixels,
    };
    let glyphs = ['/', '0', '2', '\u{fffd}'].map(|codepoint| GlyphMetrics {
        codepoint,
        page: 0,
        uv: [0, 0, 12, 16],
        bearing: [0, -14],
        advance_64: 12 * 64,
    });
    let manifest = [7; 32];
    let bytes = encode_font_catalog(manifest, &glyphs, &[page]).unwrap();
    Arc::new(RuntimeFontCatalog::decode(&bytes, manifest).unwrap())
}

/// Builds every required HUD texture with deterministic synthetic pixels.
pub fn fixture_hud() -> Arc<RuntimeHudCatalog> {
    let textures = HudTextureRole::ALL
        .into_iter()
        .map(|role| {
            let [width, height] = role.expected_size();
            let rgba8 = [role as u8, 2, 3, 255]
                .repeat(width as usize * height as usize)
                .into_boxed_slice();
            HudTexture {
                role,
                source_bytes: rgba8.len() as u32,
                source_sha256: Sha256::digest(&rgba8).into(),
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8,
            }
        })
        .collect::<Vec<_>>();
    let manifest = assets::HUD_SOURCE_MANIFEST_SHA256;
    let bytes = encode_hud_catalog(manifest, &textures).unwrap();
    Arc::new(RuntimeHudCatalog::decode(&bytes).unwrap())
}

/// A vanilla-shaped server form screen: a content factory selecting a long form
/// whose buttons come from the `form_buttons` collection.
const SERVER_FORM: &str = r##"{
  "namespace": "server_form",
  "third_party_server_screen": { "type": "screen", "$screen_content": "server_form.main_screen_content" },
  "main_screen_content": { "type": "panel", "size": [0, 0], "controls": [
    { "server_form_factory": { "type": "factory", "control_ids": { "long_form": "@server_form.long_form" } } } ] },
  "long_form": { "type": "stack_panel", "size": [200, "100%c"], "collection_name": "form_buttons",
    "factory": { "name": "buttons", "control_ids": { "button": "@server_form.form_button" } } },
  "form_button": { "type": "button", "size": ["100%", 30],
    "button_mappings": [ { "from_button_id": "button.menu_select", "to_button_id": "button.form_button_click", "mapping_type": "pressed" } ],
    "bindings": [ { "binding_type": "collection_details", "binding_collection_name": "form_buttons" } ],
    "controls": [ { "label": { "type": "label", "text": "#form_button_text", "bindings": [
      { "binding_type": "collection", "binding_collection_name": "form_buttons", "binding_name": "#form_button_text" } ] } },
      { "image": { "type": "image", "size": [16, 16], "bindings": [
        { "binding_type": "collection", "binding_collection_name": "form_buttons",
          "binding_name": "#form_button_texture", "binding_name_override": "#texture" },
        { "binding_type": "collection", "binding_collection_name": "form_buttons",
          "binding_name": "#form_button_texture_file_system", "binding_name_override": "#texture_file_system" } ] } } ] }
}"##;

/// Builds a minimal in-memory server-form carrier.
pub fn mini_carrier() -> Arc<RuntimeUiAssets> {
    let files = [
        ("ui/_global_variables.json", "{}"),
        (
            "ui/_ui_defs.json",
            r#"{ "ui_defs": ["ui/server_form.json"] }"#,
        ),
        ("ui/server_form.json", SERVER_FORM),
    ]
    .map(|(path, text)| UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    let page = UiAtlasPage {
        width: 4,
        height: 4,
        rgba8: vec![255; 64].into(),
    };
    let bytes = encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    Arc::new(RuntimeUiAssets::decode(&bytes).unwrap())
}

/// Enables the minimal carrier on a deterministic font fixture.
pub fn mini_engine_presentation() -> UiPresentationRuntime {
    let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    presentation.enable_json_ui(mini_carrier()).unwrap();
    presentation
}

/// A presentation with the fixture HUD carrier and the real UI carrier.
pub fn engine_presentation() -> Option<UiPresentationRuntime> {
    engine_presentation_with(fixture_font())
}

/// Builds an engine HUD fixture with the supplied font.
pub fn engine_presentation_with(font: Arc<RuntimeFontCatalog>) -> Option<UiPresentationRuntime> {
    let carrier = pack_harness::carrier()?;
    let mut presentation =
        UiPresentationRuntime::with_hud(font, fixture_hud()).expect("build fixture HUD");
    presentation
        .enable_json_ui(carrier)
        .expect("enable installed UI fixture");
    Some(presentation)
}

pub use crate::ui_runtime::presentation::{
    gui_models::test_support::assert_installed_geometry,
    player_preview::geometry::assert_installed_shield,
};

/// Creates a server-authoritative inventory fixture with installed translations.
pub fn inventory_session(player_runtime: &mut player_state::PlayerState) -> UiRuntime {
    *player_runtime = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(player_runtime, 1, 42)
        .unwrap();
    let lang = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("../.local/assets/compiled/vanilla-v1.mcbelang");
    if let Some(lang) = std::fs::read(lang)
        .ok()
        .and_then(|bytes| assets::RuntimeLangCatalog::decode(&bytes).ok())
    {
        runtime.set_lang_catalog(std::sync::Arc::new(lang));
    }
    runtime
}

/// Creates a creative inventory with a deterministic grouped catalog.
pub fn creative_with(player_runtime: &mut player_state::PlayerState, count: u32) -> UiRuntime {
    use protocol::{CreativeCategory, CreativeContentEvent, CreativeGroup, CreativeItem};
    let mut runtime = inventory_session(player_runtime);
    player_runtime
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Creative);
    let categories = [
        CreativeCategory::Construction,
        CreativeCategory::Nature,
        CreativeCategory::Equipment,
        CreativeCategory::Items,
    ];
    let group = |category: CreativeCategory, name: &str| CreativeGroup {
        category,
        name: name.into(),
        icon: None,
    };
    let mut groups: Vec<CreativeGroup> = categories
        .iter()
        .map(|category| group(*category, ""))
        .collect();
    groups.push(group(
        CreativeCategory::Construction,
        "itemGroup.name.planks",
    ));
    groups.push(group(
        CreativeCategory::Construction,
        "itemGroup.name.stone",
    ));
    let items = (0..count)
        .map(|index| CreativeItem {
            creative_network_id: index + 1,
            stack: protocol::NetworkItemStack {
                network_id: 1 + index as i32,
                count: 1,
                ..protocol::NetworkItemStack::empty()
            },
            group: match index {
                0..20 if index % 4 == 0 => 4,
                20..40 if index % 4 == 0 => 5,
                _ => index % 4,
            },
        })
        .collect::<Vec<_>>();
    runtime
        .inventory_ledger_mut(player_runtime)
        .apply(&protocol::InventoryEvent::Creative(CreativeContentEvent {
            groups: groups.into(),
            items: items.into(),
            skipped: 0,
        }));
    runtime.screen_state_mut().creative_expanded.insert(5);
    runtime.toggle_inventory(player_runtime);
    runtime
}

pub use crate::ui_runtime::presentation::forms::test_support::{
    draw_menu_actions, settings_view, snapshot_menu, snapshot_menu_after, snapshot_menu_at,
    snapshot_menu_vanilla,
};

pub use crate::ui_runtime::presentation::{
    forms::scene_policy::test_support::menu_settings, publish::item_icons::test_support::stack_icon,
};

pub(crate) mod play_flow;
pub use play_flow::fixture_view;

pub use crate::ui_runtime::presentation::forms::test_support::{menu_hit_targets, screen_settings};

pub use crate::ui_runtime::presentation::forms::test_support::{text_metrics, text_scale};
