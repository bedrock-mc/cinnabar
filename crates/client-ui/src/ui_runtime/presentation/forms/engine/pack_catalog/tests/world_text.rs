use super::{Draw, HudModel, Timed, layer, layer_pack_catalog, render_model, text, vanilla};

const SERVER_WORLD_TEXT: &[u8] = br#"{
    "namespace": "hud",
    "root_panel/chat_stack": {
        "size": ["100%", "100%"],
        "controls": [
            {"padding": {"type": "panel", "size": [0, 2]}},
            {"player_position@hud.player_position": {}},
            {"number_of_days_played@hud.number_of_days_played": {}}
        ]
    },
    "player_position": {"texture": "textures/ui/server_coordinates"},
    "player_position/player_position_text": {"offset": [0, -1]},
    "number_of_days_played": {"texture": "textures/ui/server_days"},
    "number_of_days_played/number_of_days_played_text": {"offset": [1, 1]}
}"#;

#[test]
fn server_world_text_style_survives_builtin_chat_restoration() {
    let Some(vanilla) = vanilla("server_world_text_style_survives_builtin_chat_restoration") else {
        return;
    };
    let pack = layer(SERVER_WORLD_TEXT);
    let model = HudModel {
        player_position: Some("Position: -17, 47, 2".into()),
        days_played: Some("Days played: 3".into()),
        text_background_alpha: 0.5,
        chat_visible: true,
        chat_lifetime: 10.0,
        chat: vec![Timed {
            text: "Chat line".into(),
            born: 0.0,
        }],
        ..Default::default()
    };
    let base = render_model(&layer_pack_catalog(&vanilla, &[]), &model, [480.0, 270.0]);
    let actual = render_model(
        &layer_pack_catalog(&vanilla, std::slice::from_ref(&pack)),
        &model,
        [480.0, 270.0],
    );
    let mut native = vanilla.clone();
    native.apply_pack(
        pack.iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    let expected = render_model(&native, &model, [480.0, 270.0]);
    for (name, label, texture) in [
        (
            "player_position",
            "Position: -17, 47, 2",
            "textures/ui/server_coordinates",
        ),
        (
            "number_of_days_played",
            "Days played: 3",
            "textures/ui/server_days",
        ),
    ] {
        let backgrounds = actual
            .nodes
            .iter()
            .filter(|node| node.name == name)
            .collect::<Vec<_>>();
        assert_eq!(backgrounds.len(), 1, "duplicate world-text background");
        assert!(
            matches!(&backgrounds[0].draw, Draw::Sprite { texture: path, .. } if path == texture),
            "chat restoration discarded the authored {name} background"
        );
        assert_eq!(
            text(&actual, label).dest,
            text(&expected, label).dest,
            "chat restoration discarded the authored {name} label offset"
        );
    }
    assert_eq!(
        text(&actual, "Chat line").dest,
        text(&base, "Chat line").dest
    );
    assert_eq!(
        text(&actual, "Chat line").draw,
        text(&base, "Chat line").draw
    );
}
