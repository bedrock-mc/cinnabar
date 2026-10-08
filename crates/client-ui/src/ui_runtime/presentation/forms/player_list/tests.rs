use super::*;
use crate::test_support::mini_engine_presentation;
use json_ui::{Draw, LayoutEnv, TextMeasure, TextureMeta, TextureSource};
use ui::DpiScale;

struct Measure;
impl TextMeasure for Measure {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}
impl TextureSource for Measure {
    fn texture(&self, _: &str) -> Option<TextureMeta> {
        None
    }
}

fn names(count: usize) -> Vec<Arc<str>> {
    (0..count)
        .map(|index| Arc::from(format!("Player{index:02}")))
        .collect()
}

#[test]
fn list_fills_down_columns_and_never_shows_padding_as_a_player() {
    let mut list = PlayerList::new();
    assert_eq!(list.refresh(&names(23), MAX_ROWS), [2, 12]);
    let context = Context::default()
        .with_var("row_height", serde_json::json!(ROW_HEIGHT))
        .with_var("row_background_height", serde_json::json!(ROW_HEIGHT - 1.0))
        .with_var("grid_top", serde_json::json!(ROW_HEIGHT + 3.0))
        .with_var("cell_width", serde_json::json!(80))
        .with_var("panel_width", serde_json::json!(164))
        .with_var("panel_height", serde_json::json!(122));
    let env = LayoutEnv {
        text: &Measure,
        textures: &Measure,
    };
    let rendered = list
        .screen
        .render_shared_with(
            SCREEN,
            &list.catalog,
            &context,
            Arc::clone(&list.data),
            ([320.0, 240.0], 1.0, [0; 3]),
            &env,
            &ViewState::default(),
        )
        .unwrap();
    let labels: Vec<_> = rendered
        .nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } if text.starts_with("Player") => {
                Some((text.as_str(), node.dest))
            }
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 23);
    let find = |name: &str| labels.iter().find(|(text, _)| *text == name).unwrap().1;
    assert_eq!(find("Player00").x, find("Player01").x);
    assert!(find("Player01").y > find("Player00").y);
    assert!(find("Player12").x > find("Player00").x);
    assert_eq!(find("Player12").y, find("Player00").y);
}

#[test]
fn bounded_roster_and_unchanged_controller_share_the_same_data() {
    let mut list = PlayerList::new();
    assert_eq!(list.refresh(&names(100), MAX_ROWS), [4, 20]);
    assert_eq!(list.names.len(), MAX_PLAYERS);
    assert_eq!(list.total, 100);
    let before = Arc::clone(&list.data);
    list.refresh(&names(100), MAX_ROWS);
    assert!(Arc::ptr_eq(&before, &list.data));
    list.refresh(&names(1), MAX_ROWS);
    assert_eq!(list.dimensions, [1, 1]);
    list.refresh(&[], MAX_ROWS);
    assert_eq!(list.dimensions, [1, 1]);
    assert!(list.names.is_empty());
}

#[test]
fn hold_release_roster_changes_and_chat_focus_drive_the_rendered_overlay() {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&super::super::ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".into(),
                br#"{"namespace":"hud","hud_screen":{"type":"screen","controls":[]}}"#.to_vec(),
            ),
        ]],
        ..Default::default()
    });
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.refresh_raw_text_identities(|_| None, names(2));
    let frame = |presentation: &mut UiPresentationRuntime,
                 player: &player_state::PlayerState,
                 runtime: &UiRuntime| {
        presentation
            .build(player, runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
            .unwrap()
    };
    let hidden = frame(&mut presentation, &player, &runtime);
    runtime.set_player_list_held(true);
    let shown = frame(&mut presentation, &player, &runtime);
    assert_ne!(hidden.vertices, shown.vertices);
    assert_eq!(shown, frame(&mut presentation, &player, &runtime));
    assert_eq!(
        presentation
            .form_presentation
            .player_list
            .as_ref()
            .unwrap()
            .screen
            .passes,
        1
    );
    runtime.refresh_raw_text_identities(|_| None, names(3));
    let changed = frame(&mut presentation, &player, &runtime);
    assert_ne!(shown.vertices, changed.vertices);
    runtime.set_player_list_held(false);
    let released = frame(&mut presentation, &player, &runtime);
    assert_eq!(hidden.vertices, released.vertices);
    assert_eq!(hidden.indices, released.indices);
    runtime.set_player_list_held(true);
    runtime.open_chat(&mut player);
    let passes = presentation
        .form_presentation
        .player_list
        .as_ref()
        .unwrap()
        .screen
        .passes;
    frame(&mut presentation, &player, &runtime);
    assert_eq!(
        presentation
            .form_presentation
            .player_list
            .as_ref()
            .unwrap()
            .screen
            .passes,
        passes
    );
    runtime.begin_session(2);
    assert!(!runtime.player_list_held());
    assert!(runtime.known_player_names().is_empty());
}
