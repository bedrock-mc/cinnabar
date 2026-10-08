use super::*;
use crate::ui_runtime::SequencedLocalAttributes;

struct Metrics;

impl json_ui::TextMeasure for Metrics {
    fn extent(&self, _: &str) -> [f64; 2] {
        [0.0, 0.0]
    }
}

impl json_ui::TextureSource for Metrics {
    fn texture(&self, _: &str) -> Option<json_ui::TextureMeta> {
        None
    }
}

/// Rebind the same HUD controls while toggling a pack's hunger visibility.
fn bind_hunger(screens: &mut HudScreens, visible: bool) {
    let catalog = screens.hud.resolved.as_ref().map(|resolved| Arc::clone(&resolved.catalog)).unwrap_or_else(|| {
        let source = r##"{
            "namespace":"hud",
            "root":{"type":"panel","controls":[
                {"food":{"type":"custom","size":[1,1],"renderer":"$food_renderer",
                    "bindings":[{"binding_name":"#food_visible","binding_name_override":"#visible"}]}},
                {"other":{"type":"custom","size":[1,1],"renderer":"armor_renderer"}}
            ]}
        }"##
        .replace(
            "$food_renderer",
            super::super::hud_renderers::HUNGER_RENDERER,
        );
        Arc::new(
            Catalog::from_files([
                ("ui/_global_variables.json", b"{}".as_slice()),
                (
                    "ui/_ui_defs.json",
                    br#"{"ui_defs":["ui/hud.json"]}"#.as_slice(),
                ),
                ("ui/hud.json", source.as_bytes()),
            ])
            .unwrap(),
        )
    });
    let mut data = DataSource::default();
    data.set_global("#food_visible", json_ui::Scalar::Bool(visible));
    screens
        .hud
        .render(
            "hud.root",
            &catalog,
            &Context::desktop(),
            Arc::new(data),
            ([480.0, 270.0], 1.0, [0; 3]),
            &json_ui::LayoutEnv {
                text: &Metrics,
                textures: &Metrics,
            },
        )
        .unwrap();
}

/// Publish a food update without advancing the server tick.
fn food_update(player: &mut player_state::PlayerState, runtime: &mut UiRuntime, sequence: u64) {
    runtime
        .apply_local_attributes(
            player,
            SequencedLocalAttributes {
                session_id: runtime.session_id(),
                fifo_sequence: sequence,
                local_millis: sequence * 10,
                server_tick: 0,
                attributes: Arc::from([
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:player.hunger"),
                        min: 0.0,
                        max: 20.0,
                        current: 18.0,
                        default: None,
                        modifiers: Arc::from([]),
                    },
                    protocol::ActorAttribute {
                        name: Arc::from("minecraft:player.saturation"),
                        min: 0.0,
                        max: 20.0,
                        current: 0.0,
                        default: None,
                        modifiers: Arc::from([]),
                    },
                ]),
            },
        )
        .unwrap();
}

#[test]
fn repeated_zero_tick_food_updates_do_not_restart_hunger_motion() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut screens = HudScreens::default();
    let mut frame = HudFrame::default();
    let options = Default::default();
    bind_hunger(&mut screens, true);
    for update in 1..=110 {
        food_update(&mut player, &mut runtime, update);
        frame.now_millis = update * 10;
        let paint = screens.capture_status(false, &player, &runtime, &frame, None, &options);
        if update % 55 == 0 {
            assert!(paint.hunger.iter().any(|cell| cell.at[1] == -1.0));
            assert!(
                paint
                    .hunger
                    .iter()
                    .all(|cell| [-1.0, 0.0].contains(&cell.at[1]))
            );
        } else {
            assert!(
                paint.hunger.iter().all(|cell| cell.at[1] == 0.0),
                "food packets must not start another shake at update {update}"
            );
        }
        for layers in paint.hunger.chunks_exact(2) {
            assert_eq!(layers[0].at, layers[1].at);
        }
        screens.finish_status(false);
        screens.capture_status(true, &player, &runtime, &frame, None, &options);
        screens.finish_status(true);
    }
}

#[test]
fn a_new_session_starts_with_a_neutral_hunger_row() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut screens = HudScreens::default();
    let frame = HudFrame::default();
    let options = Default::default();
    bind_hunger(&mut screens, true);
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        screens.capture_status(false, &player, &runtime, &frame, None, &options);
        screens.finish_status(false);
    }
    let mut player = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    food_update(&mut player, &mut runtime, 1);
    let paint = screens.capture_status(false, &player, &runtime, &frame, None, &options);
    assert!(!paint.hunger.is_empty());
    assert!(paint.hunger.iter().all(|cell| cell.at[1] == 0.0));
}

#[test]
fn a_pack_hidden_hunger_renderer_does_not_advance_its_pulse() {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let mut screens = HudScreens::default();
    let frame = HudFrame::default();
    let options = Default::default();
    food_update(&mut player, &mut runtime, 1);
    bind_hunger(&mut screens, true);
    for _ in 0..54 {
        screens.capture_status(false, &player, &runtime, &frame, None, &options);
        screens.finish_status(false);
    }
    bind_hunger(&mut screens, false);
    assert!(
        screens.hud.has_visible_content(),
        "the rest of the HUD stays visible"
    );
    for _ in 0..100 {
        screens.capture_status(false, &player, &runtime, &frame, None, &options);
        screens.finish_status(false);
    }
    bind_hunger(&mut screens, true);
    let paint = screens.capture_status(false, &player, &runtime, &frame, None, &options);
    assert!(
        paint.hunger.iter().any(|cell| cell.at[1] == -1.0),
        "the next visible hunger render must resume its pending pulse"
    );
}
