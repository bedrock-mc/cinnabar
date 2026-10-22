use super::*;
use mod_ui::PanelUiOwner;
use serde_json::json;
use ui::mod_panel::{Surface, SurfaceTransition, SurfaceValue};

#[derive(Clone, Copy, Debug)]
enum Cursor {
    Native,
    Pack,
    Custom,
}

/// A deterministic HUD with an ordinary, pack-image or personal crosshair.
fn presentation(cursor: Cursor) -> UiPresentationRuntime {
    let mut presentation =
        UiPresentationRuntime::with_hud(fixture_font(), crate::test_support::fixture_hud())
            .unwrap();
    presentation
        .enable_json_ui(crate::test_support::mini_carrier())
        .unwrap();
    presentation.hud_frame_mut().first_person = true;
    if matches!(cursor, Cursor::Custom) {
        presentation
            .mod_ui_mut()
            .set_mod_crosshair(Some(&ui::mod_hud::Crosshair {
                color: [1., 0., 1., 1.],
                ..Default::default()
            }))
            .unwrap();
    }
    presentation
}

/// Publishes transparent screens so scene absorption alone decides cursor visibility.
fn runtime(cursor: Cursor) -> UiRuntime {
    let (namespace, screen) = json_ui::vanilla::Reference::HudCrosshairHudCrosshairScreen
        .as_str()
        .split_once('.')
        .unwrap();
    let control = if matches!(cursor, Cursor::Pack) {
        json!({"type":"image","texture":"textures/ui/menu_test_cursor","size":[13,13]})
    } else {
        json!({"type":"custom","renderer":"cursor_renderer","size":[ui_screens::hud_renderers::CROSSHAIR_SIDE,ui_screens::hud_renderers::CROSSHAIR_SIDE]})
    };
    let crosshair = json!({"namespace":namespace,(screen):{
        "type":"screen","absorbs_input":false,"is_showing_menu":false,"render_only_when_topmost":false,"render_game_behind":true,
        "should_steal_mouse":false,"controls":[{"cursor":control}]
    }});
    let hud = json!({"namespace":"hud","hud_screen":{
        "type":"screen","absorbs_input":false,"is_showing_menu":false,"render_only_when_topmost":false,"render_game_behind":true,
        "should_steal_mouse":true,"controls":[{"marker":{
            "type":"label","text":"Other HUD stays visible","localize":false,"color":[0,1,0]
        }}]
    }});
    let chat = json!({"namespace":"chat","chat_screen":{
        "type":"screen","absorbs_input":true,"render_game_behind":true,
        "controls":[]
    }});
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(13, 13, image::Rgba([29, 83, 241, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut runtime = UiRuntime::new(1);
    runtime.set_server_ui(Some(Arc::new(super::super::super::forms::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                json!({"ui_defs":[
                    "ui/menu_test_crosshair.json",
                    "ui/menu_test_hud.json",
                    "ui/menu_test_chat.json"
                ]})
                .to_string()
                .into_bytes(),
            ),
            (
                "ui/menu_test_crosshair.json".into(),
                crosshair.to_string().into_bytes(),
            ),
            ("ui/menu_test_hud.json".into(), hud.to_string().into_bytes()),
            (
                "ui/menu_test_chat.json".into(),
                chat.to_string().into_bytes(),
            ),
        ]],
        textures: vec![("textures/ui/menu_test_cursor.png".into(), png)],
        ..Default::default()
    })));
    runtime
}

/// Detects the cursor's own painted output, without inspecting its authored tree.
fn has_cursor(presentation: &UiPresentationRuntime, cursor: Cursor) -> bool {
    presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .any(|node| match (cursor, node.visual()) {
            (
                Cursor::Native,
                ui::UiVisual::Sprite { uv, .. } | ui::UiVisual::InvertedSprite { uv, .. },
            ) => {
                let [width, height] = assets::HudTextureRole::Crosshair.expected_size();
                uv[2] - uv[0] == width as u16 && uv[3] - uv[1] == height as u16
            }
            (Cursor::Pack, ui::UiVisual::Sprite { uv, .. }) => {
                uv[2] - uv[0] == 13 && uv[3] - uv[1] == 13
            }
            (Cursor::Custom, ui::UiVisual::Mesh(mesh)) => mesh
                .vertices()
                .iter()
                .any(|vertex| vertex.color[..3] == [255, 0, 255]),
            _ => false,
        })
}

/// A hidden replacement must not reveal the ordinary fallback cursor either.
fn has_any_cursor(presentation: &UiPresentationRuntime) -> bool {
    [Cursor::Native, Cursor::Pack, Cursor::Custom]
        .into_iter()
        .any(|cursor| has_cursor(presentation, cursor))
}

fn cursor_diagnostics(
    presentation: &UiPresentationRuntime,
    runtime: &UiRuntime,
    player: &player_state::PlayerState,
) -> String {
    let stack = runtime.scenes_in(
        player,
        presentation.scene_host(),
        &presentation.screen_settings(),
    );
    let paint = hud_layout::capture_hud_paint(
        player,
        runtime,
        &presentation.hud_frame,
        presentation.hud_textures.as_ref(),
        &launcher::menu::settings_options::SettingsOptions::default(),
    );
    format!(
        "scenes={:?}; visible={:?}; targets={:?}; first_person={}; cursor={:?}; visuals={:?}",
        stack.scenes(),
        stack.visible(false),
        stack.input_targets(),
        presentation.hud_frame.first_person,
        paint.crosshair,
        presentation
            .last_frame
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .map(|node| node.visual())
            .collect::<Vec<_>>(),
    )
}

/// The crosshair requires first-person gameplay input and a non-spectator mode.
#[test]
fn crosshair_is_first_person_only_and_mode_gated() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = presentation(Cursor::Native);
    presentation.hud_frame_mut().first_person = false;
    let mut runtime = runtime(Cursor::Native);
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let paint = |player_runtime: &player_state::PlayerState,
                 presentation: &UiPresentationRuntime,
                 runtime: &UiRuntime| {
        hud_layout::capture_hud_paint(
            player_runtime,
            runtime,
            presentation.hud_frame(),
            presentation.hud_textures.as_ref(),
            &Default::default(),
        )
    };
    assert!(
        paint(&player_runtime, &presentation, &runtime)
            .crosshair
            .is_none(),
        "third person"
    );
    *presentation.hud_frame_mut() = first_person();
    assert!(
        crosshair(
            &build(&player_runtime, &mut presentation, &runtime, 0),
            30.0
        )
        .is_some()
    );
    runtime.open_chat(&mut player_runtime);
    assert!(
        crosshair(
            &build(&player_runtime, &mut presentation, &runtime, 0),
            30.0
        )
        .is_none(),
        "chat owns input"
    );
    runtime.close_chat();
    assert!(
        crosshair(
            &build(&player_runtime, &mut presentation, &runtime, 0),
            30.0
        )
        .is_some(),
        "closing chat restores gameplay input"
    );
    player_runtime
        .facts
        .publish_player_game_mode(PlayerGameMode::Spectator);
    assert!(
        paint(&player_runtime, &presentation, &runtime)
            .crosshair
            .is_none(),
        "spectator"
    );
}

#[test]
fn crosshair_menu_input_ownership_hides_every_style_and_restores_after_nested_close() {
    for cursor in [Cursor::Native, Cursor::Pack, Cursor::Custom] {
        let mut presentation = presentation(cursor);
        let mut runtime = runtime(cursor);
        let mut player = player_state::PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Survival);
        build(&player, &mut presentation, &runtime, 0);
        assert!(
            has_cursor(&presentation, cursor),
            "baseline {cursor:?}: {}",
            cursor_diagnostics(&presentation, &runtime, &player)
        );
        runtime.open_chat(&mut player);
        build(&player, &mut presentation, &runtime, 1);
        assert!(
            !has_any_cursor(&presentation),
            "chat owns input: {cursor:?}"
        );
        assert!(presentation.last_frame.as_ref().unwrap().nodes.iter().any(|node| {
            matches!(node.visual(), ui::UiVisual::Text { color, .. } if *color == [0,255,0,255])
        }));
        // Closing the personal panel cannot uncover a cursor while chat still owns input.
        presentation
            .mod_ui_mut()
            .set_mod_panel(
                PanelUiOwner::Environment(0),
                Some(&mod_ui::test_support::sample_panel()),
            )
            .unwrap();
        presentation.mod_ui_mut().set_mod_panel_open(true);
        build(&player, &mut presentation, &runtime, 2);
        assert!(!has_any_cursor(&presentation));
        presentation.mod_ui_mut().set_mod_panel_open(false);
        build(&player, &mut presentation, &runtime, 3);
        assert!(
            !has_any_cursor(&presentation),
            "chat remains open: {cursor:?}"
        );
        runtime.close_chat();
        build(&player, &mut presentation, &runtime, 4);
        assert!(
            has_cursor(&presentation, cursor),
            "last menu closed: {cursor:?}"
        );
    }
}

#[test]
fn crosshair_menu_panel_close_restores_cursor_while_exit_is_still_painted() {
    let mut presentation = presentation(Cursor::Native);
    let runtime = runtime(Cursor::Native);
    let mut player = player_state::PlayerState::new(1);
    player
        .facts
        .publish_player_game_mode(PlayerGameMode::Survival);
    let mut panel = mod_ui::test_support::sample_panel();
    panel.motion = true;
    panel.surface = Some(Surface {
        collections: Default::default(),
        previews: Default::default(),
        screen: "extension.cursor_exit".into(),
        document: json!({"namespace":"extension","cursor_exit":{
            "type":"screen","controls":[{"body":{"type":"panel","size":[40,20],
                "bindings":[{"binding_name":"#fade","binding_name_override":"#alpha"}]
            }}]
        }})
        .to_string(),
        bindings: std::collections::BTreeMap::from([("#fade".into(), SurfaceValue::Number(1.))]),
        transitions: std::collections::BTreeMap::from([(
            "#fade".into(),
            SurfaceTransition {
                duration: 1.,
                easing: Default::default(),
                from: None,
                exit: Some(SurfaceValue::Number(0.)),
            },
        )]),
    });
    presentation
        .mod_ui_mut()
        .set_mod_panel(PanelUiOwner::Environment(0), Some(&panel))
        .unwrap();
    presentation.mod_ui_mut().set_mod_panel_open(true);
    build(&player, &mut presentation, &runtime, 0);
    assert!(!has_any_cursor(&presentation));
    presentation.mod_ui_mut().set_mod_panel_open(false);
    build(&player, &mut presentation, &runtime, 1);
    assert!(!presentation.mod_ui().mod_panel_open());
    assert!(
        presentation.mod_ui().mod_panel_painting(),
        "exit retains its surface"
    );
    assert!(
        has_cursor(&presentation, Cursor::Native),
        "gameplay input restores immediately: {}",
        cursor_diagnostics(&presentation, &runtime, &player)
    );
}
