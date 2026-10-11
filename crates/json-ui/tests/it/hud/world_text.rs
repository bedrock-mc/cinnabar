use super::{
    Catalog, Context, DrawNode, FixedText, HUD_SCREEN, HudModel, LayoutEnv, PackTextures,
    ViewState, hud_context, hud_data_source, java_pack, named, pack, render_screen,
};

const POSITION: &str = "Position: -17, 47, 2";
const DAYS: &str = "Days played: 3";

const STACK_CONTENTS: &[u8] = br#"{
    "root_panel/chat_stack": {
        "size": ["100%", "100%"],
        "controls": [
            {"padding": {"type": "panel", "size": [0, 2]}},
            {"player_position@hud.player_position": {}},
            {"number_of_days_played@hud.number_of_days_played": {}}
        ]
    }
}"#;

const PANEL_CONTENTS: &[u8] = br#"{
    "root_panel/chat_stack": {
        "type": "panel",
        "size": ["100%", "100%"],
        "controls": [
            {"player_position@hud.player_position": {
                "anchor_from": "bottom_right", "anchor_to": "bottom_right",
                "offset": [-6, -18]
            }},
            {"number_of_days_played@hud.number_of_days_played": {
                "anchor_from": "bottom_right", "anchor_to": "bottom_right",
                "offset": [-6, -3]
            }}
        ]
    }
}"#;

/// Builds native and built-in HUD catalogs with the same partial server overlay.
fn catalogs(test: &str, patch: &[u8]) -> Option<(Catalog, Catalog, PackTextures)> {
    let Some(directory) = pack() else {
        eprintln!("skipping {test}: missing pinned vanilla resource pack; make assets");
        return None;
    };
    let mut layered = Catalog::load_dir(&directory.join("ui")).expect("vanilla UI loads");
    let mut native = layered.clone();
    native.apply_pack([("ui/hud_screen.json", patch)]);
    let builtin = java_pack::files();
    layered.apply_pack(
        builtin
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    layered.apply_pack([("ui/hud_screen.json", patch)]);
    Some((native, layered, PackTextures::new(directory)))
}

/// Renders the bound world-text lines at a requested virtual viewport size.
fn render_world_text(
    catalog: &Catalog,
    model: &HudModel,
    viewport: [f64; 2],
    textures: &PackTextures,
) -> Vec<DrawNode> {
    render_screen(
        HUD_SCREEN,
        catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(model),
        viewport,
        &LayoutEnv {
            text: &FixedText,
            textures,
        },
        &ViewState::default(),
    )
    .expect("HUD renders")
    .nodes
}

#[test]
fn server_world_text_controls_keep_native_vertical_packing() {
    let Some((native, layered, textures)) = catalogs(
        "server_world_text_controls_keep_native_vertical_packing",
        STACK_CONTENTS,
    ) else {
        return;
    };
    let model = HudModel {
        player_position: Some(POSITION.into()),
        days_played: Some(DAYS.into()),
        ..Default::default()
    };
    for viewport in [[480.0, 270.0], [960.0, 540.0]] {
        let expected = render_world_text(&native, &model, viewport, &textures);
        let actual = render_world_text(&layered, &model, viewport, &textures);
        for name in ["player_position_text", "number_of_days_played_text"] {
            let expected = named(&expected, name);
            let actual = named(&actual, name);
            assert_eq!(expected.len(), 1);
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].dest, expected[0].dest, "{name} at {viewport:?}");
            assert_eq!(actual[0].draw, expected[0].draw);
        }
        let position = named(&actual, "player_position_text")[0];
        let days = named(&actual, "number_of_days_played_text")[0];
        assert!(
            position.dest.y < 12.0,
            "coordinates moved below the top row"
        );
        assert!(days.dest.y >= position.dest.y + position.dest.h);
        let hidden = render_world_text(&layered, &HudModel::default(), viewport, &textures);
        assert!(named(&hidden, "player_position_text").is_empty());
        assert!(named(&hidden, "number_of_days_played_text").is_empty());
    }
}

#[test]
fn explicit_server_world_text_panel_keeps_authored_anchors() {
    let Some((native, layered, textures)) = catalogs(
        "explicit_server_world_text_panel_keeps_authored_anchors",
        PANEL_CONTENTS,
    ) else {
        return;
    };
    let model = HudModel {
        player_position: Some(POSITION.into()),
        days_played: Some(DAYS.into()),
        ..Default::default()
    };
    for viewport in [[480.0, 270.0], [960.0, 540.0]] {
        let expected = render_world_text(&native, &model, viewport, &textures);
        let actual = render_world_text(&layered, &model, viewport, &textures);
        for name in ["player_position_text", "number_of_days_played_text"] {
            let expected = named(&expected, name);
            let actual = named(&actual, name);
            assert_eq!(expected.len(), 1);
            assert_eq!(actual.len(), 1);
            assert_eq!(actual[0].dest, expected[0].dest, "{name} at {viewport:?}");
            assert!(actual[0].dest.x > viewport[0] * 0.5);
            assert!(actual[0].dest.y > viewport[1] * 0.5);
        }
    }
}
