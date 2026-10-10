use std::sync::Arc;

use json_ui::{Draw, DrawNode};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use player_state::PlayerState;
use protocol::{TextCategory, TextEvent, TextKind, UiEvent};
use ui::DpiScale;

use crate::test_support::engine_presentation;
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};
use {
    super::*,
    launcher::menu::settings_options::{CHAT_POSITION_OPTION, SettingsOptions},
};

fn option_index() -> usize {
    launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == CHAT_POSITION_OPTION.name)
        .unwrap()
}

fn visible_text<'a>(nodes: &'a [DrawNode], wanted: &str) -> &'a DrawNode {
    nodes
        .iter()
        .find(|node| {
            node.alpha > 0.0 && matches!(&node.draw, Draw::Text { text, .. } if text == wanted)
        })
        .expect("visible text")
}

#[test]
fn chat_position_switch_repositions_retained_hud_and_focused_history() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping chat position layout: missing local UI carrier (make assets)");
        return;
    };
    let mut player = PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime
        .apply(
            &mut player,
            SequencedUiEvent {
                session_id: 1,
                fifo_sequence: 1,
                local_millis: 0,
                server_tick: None,
                event: UiEvent::Text(TextEvent {
                    category: TextCategory::MessageOnly,
                    kind: TextKind::Chat,
                    needs_translation: false,
                    source: None,
                    message: Arc::from("placement proof"),
                    parameters: Arc::from([]),
                    xuid: Arc::from(""),
                    platform_chat_id: Arc::from(""),
                    filtered_message: None,
                }),
            },
        )
        .unwrap();
    let mut settings = SettingsOptions::default();
    for top in [false, true, false] {
        settings.set(
            option_index(),
            if top {
                CHAT_POSITION_OPTION.max
            } else {
                CHAT_POSITION_OPTION.default
            },
        );
        presentation.set_chat_settings_snapshot((Arc::new(settings.clone()), None));
        runtime.close_chat();
        for (size, dpi) in [([1280, 720], 1.0), ([1920, 1080], 2.0)] {
            presentation
                .build(&player, &runtime, 0, size, DpiScale::new(dpi).unwrap())
                .unwrap();
            let hud = visible_text(presentation.hud_draw_nodes(), "placement proof");
            assert_eq!(
                hud.dest.y < 80.0,
                top,
                "HUD at {size:?}, DPI {dpi}: {:?}",
                hud.dest
            );
            assert!(hud.dest.x >= 0.0 && hud.dest.y >= 0.0);
        }
        runtime.open_chat(&mut player);
        runtime.insert_chat_text("draft").unwrap();
        presentation
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let history = visible_text(presentation.chat_draw_nodes(), "placement proof");
        assert_eq!(
            history.dest.y < 120.0,
            top,
            "focused history follows the preference: {:?}",
            history.dest
        );
        let draft = visible_text(presentation.chat_draw_nodes(), "draft|");
        assert!(draft.dest.y > history.dest.y + history.dest.h);
        runtime.close_chat();
    }
    settings.set(option_index(), CHAT_POSITION_OPTION.max);
    presentation.set_chat_settings_snapshot((Arc::new(settings), None));
    runtime.apply_hud_rules(protocol::HudRules {
        show_coordinates: Some(true),
        show_days_played: Some(true),
    });
    presentation.hud_frame_mut().player_block = Some([12, 64, -7]);
    presentation.hud_frame_mut().world_time = Some(24_000.0 * 3.0);
    presentation
        .build(
            &player,
            &runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    let nodes = presentation.hud_draw_nodes();
    let history = visible_text(nodes, "placement proof");
    let days = visible_text(nodes, "Days played: 3");
    assert!(
        history.dest.y >= days.dest.y + days.dest.h,
        "top chat clears the world labels"
    );
    assert_eq!(
        nodes
            .iter()
            .filter(|node| node.alpha > 0.0
                && matches!(&node.draw, Draw::Text { text, .. } if text == "Days played: 3"))
            .count(),
        1,
        "padding does not redraw the world text"
    );
}

#[test]
fn chat_position_dropdown_rows_select_their_own_choice() {
    let Some(mut presentation) = engine_presentation() else {
        eprintln!("skipping chat position dropdown: missing local UI carrier (make assets)");
        return;
    };
    let player = PlayerState::new(1);
    let mut view = MenuView::new(true, "Steve".into());
    view.screen = MenuScreen::Settings;
    view.settings_section = super::super::menu_screens::SETTINGS_SECTIONS
        .iter()
        .find_map(|(name, index)| (*name == "video_forced_index").then_some(*index))
        .unwrap();
    view.settings_dropdown = Some(option_index() as u16);
    presentation.set_menu_view(Some(view));
    presentation
        .build(
            &player,
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    for choice in CHAT_POSITION_OPTION.min..=CHAT_POSITION_OPTION.max {
        let action = MenuAction::SettingsOption(option_index() as u16, choice);
        let bounds = presentation
            .menu_hit_targets
            .iter()
            .find_map(|(target, bounds)| (*target == action).then_some(*bounds))
            .expect("clickable choice");
        let point = ui::UiPoint::new(
            (bounds.min().x() + bounds.max().x()) / 2.0,
            (bounds.min().y() + bounds.max().y()) / 2.0,
        )
        .unwrap();
        assert_eq!(presentation.hit_test_menu(point), Some(action));
    }
}
