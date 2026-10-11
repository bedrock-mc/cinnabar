use std::sync::Arc;

use json_ui::{Draw, DrawNode};
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use player_state::PlayerState;
use protocol::{TextCategory, TextEvent, TextKind, UiEvent};
use sha2::{Digest, Sha256};
use ui::DpiScale;

use crate::test_support::engine_presentation;
use crate::ui_runtime::presentation::UiPresentationRuntime;
use crate::ui_runtime::{SequencedUiEvent, UiRuntime};
use launcher::menu::settings_options::{CHAT_POSITION_OPTION, SettingsOptions};

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

/// Reads actual glyph and shadow vertices while retaining the painted label's clip ancestors.
fn painted_text_bounds(
    presentation: &UiPresentationRuntime,
    wanted: &str,
    size: [u32; 2],
    dpi: DpiScale,
) -> ui::UiRect {
    let content_sha256: [u8; 32] = Sha256::digest(wanted.as_bytes()).into();
    let label = presentation
        .assembly_nodes
        .iter()
        .find(|node| {
            matches!(node.visual(), ui::UiVisual::Text { layout, color, .. }
                if color[3] > 0 && layout.key().content_sha256 == content_sha256)
        })
        .expect("message has a painted label");
    let mut nodes = vec![label.clone()];
    let mut parent = label.parent();
    while let Some(id) = parent {
        let ancestor = presentation
            .assembly_nodes
            .iter()
            .find(|node| node.id() == id)
            .expect("painted label ancestor");
        nodes.push(ancestor.clone().with_visual(ui::UiVisual::None));
        parent = ancestor.parent();
    }
    let viewport = ui::UiRect::new(
        ui::UiPoint::new(0.0, 0.0).unwrap(),
        ui::UiPoint::new(size[0] as f32 / dpi.get(), size[1] as f32 / dpi.get()).unwrap(),
    )
    .unwrap();
    let mut tree = ui::UiTree::new(nodes).unwrap();
    tree.layout(viewport, ui::UiScale::default(), presentation.safe_area)
        .unwrap();
    let drawn = tree.build_draw_list().unwrap();
    assert!(!drawn.indices.is_empty(), "message emits visible glyphs");
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for batch in &drawn.batches {
        for index in
            &drawn.indices[batch.index_range.start as usize..batch.index_range.end as usize]
        {
            let position = drawn.vertices[*index as usize].position;
            let point = ui::UiPoint::new(position[0], position[1]).unwrap();
            assert!(
                viewport.contains(point) && batch.clip.contains(point),
                "message glyph or shadow {position:?} falls outside {:?}",
                batch.clip
            );
            for axis in 0..2 {
                min[axis] = min[axis].min(position[axis]);
                max[axis] = max[axis].max(position[axis]);
            }
        }
    }
    ui::UiRect::new(
        ui::UiPoint::new(min[0], min[1]).unwrap(),
        ui::UiPoint::new(max[0], max[1]).unwrap(),
    )
    .unwrap()
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
            let dpi_scale = DpiScale::new(dpi).unwrap();
            presentation
                .build(&player, &runtime, 0, size, dpi_scale)
                .unwrap();
            let ink = painted_text_bounds(&presentation, "placement proof", size, dpi_scale);
            let px = ui::gui_scale(size, presentation.gui_scale_preference) as f32 / dpi;
            assert_eq!(
                ink.min().y() / px < 80.0,
                top,
                "painted HUD at {size:?}, DPI {dpi}: {ink:?}"
            );
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
    let size = [1280, 720];
    let dpi = DpiScale::new(1.0).unwrap();
    presentation.build(&player, &runtime, 0, size, dpi).unwrap();
    let nodes = presentation.hud_draw_nodes();
    let history_ink = painted_text_bounds(&presentation, "placement proof", size, dpi);
    let px = ui::gui_scale(size, presentation.gui_scale_preference) as f32 / dpi.get();
    let mut world_bottom: f32 = 0.0;
    for name in ["player_position", "number_of_days_played"] {
        let background = nodes
            .iter()
            .find(|node| {
                node.alpha > 0.0 && node.name == name && matches!(node.draw, Draw::Sprite { .. })
            })
            .expect("visible world-label background");
        let label_name = format!("{name}_text");
        let text = nodes
            .iter()
            .find_map(|node| match &node.draw {
                Draw::Text { text, .. } if node.alpha > 0.0 && node.name == label_name => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .expect("visible world-label text");
        let ink = painted_text_bounds(&presentation, text, size, dpi);
        // Image destinations use GUI units; emitted text vertices use logical pixels.
        let background_bottom = (background.dest.y + background.dest.h) as f32 * px;
        world_bottom = world_bottom.max(background_bottom).max(ink.max().y());
    }
    assert!(
        history_ink.min().y() >= world_bottom,
        "painted top chat {history_ink:?} overlaps the world labels ending at {world_bottom}"
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
