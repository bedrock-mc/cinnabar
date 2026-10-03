//! Reference checks against the real vanilla screens; run explicitly after fetching the pack.

mod support;

use json_ui::{Catalog, Context, ResolvedControl};

/// Required authored controls identify successful resolution without freezing tree size.
const REQUIRED_CONTROLS: &[(&str, &[&str])] = &[
    (
        "start.start_screen",
        &["achievements_button", "buy_game_button"],
    ),
    ("play.play_screen", &["add_server_button"]),
    (
        "settings.screen_controls_and_settings",
        &["available_pack_grid"],
    ),
    ("pause.pause_screen", &["root_screen_panel"]),
    ("hud.hud_screen", &["boss_health_grid", "chat_panel"]),
    ("crafting.inventory_screen", &["armor_grid"]),
    ("chest.small_chest_screen", &["chest_label"]),
    ("server_form.long_form", &["inside_header_panel"]),
    ("server_form.custom_form", &["common_panel"]),
    (
        "popup_dialog.modal_dialog_popup",
        &["background_with_buttons"],
    ),
];

fn catalog() -> Option<Catalog> {
    let dir = support::vanilla_pack().join("ui");
    dir.is_dir()
        .then(|| Catalog::load_dir(&dir).expect("index files load"))
}

fn has(control: &ResolvedControl, name: &str) -> bool {
    control.find(&|node| node.name == name).is_some()
}

/// Key vanilla screens resolve with their expected functional controls.
#[test]
#[ignore = "requires the pinned local vanilla UI pack; fetch vanilla-assets first"]
fn key_screens_resolve_to_their_known_shape() {
    let Some(catalog) = catalog() else {
        panic!("requires the pinned local vanilla UI pack; fetch vanilla-assets first");
    };
    let context = Context::retail(true);
    for (reference, names) in REQUIRED_CONTROLS {
        let root = json_ui::resolve(&catalog, reference, &context)
            .control
            .unwrap_or_else(|| panic!("{reference} resolves"));
        for name in *names {
            assert!(has(&root, name), "{reference} lacks {name}");
        }
    }
}
