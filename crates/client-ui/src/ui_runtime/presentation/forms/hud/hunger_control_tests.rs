use super::super::engine::FormEngine;
use super::super::server_pack::ServerAtlas;
use super::*;
use crate::test_support::{fixture_font, mini_carrier};
use crate::ui_runtime::SequencedLocalAttributes;
use ui::{DpiScale, SafeArea, TextLayoutCache};

/// A server pack placing independently visible hunger controls on separate rows.
fn engine() -> FormEngine {
    let mut catalog = Catalog::default();
    catalog.overlay_text("ui/hunger_test.json", &serde_json::json!({
        "namespace":"hud", "root":{"type":"panel", "controls":[
            {"first":{"type":"custom", "size":[1,1], "offset":[160,30],
                "anchor_from":"top_left", "anchor_to":"top_left",
                "renderer":super::super::hud_renderers::HUNGER_RENDERER,
                "bindings":[{"binding_name":"#first_visible", "binding_name_override":"#visible"}]}},
            {"second":{"type":"custom", "size":[1,1], "offset":[160,60],
                "anchor_from":"top_left", "anchor_to":"top_left",
                "renderer":super::super::hud_renderers::HUNGER_RENDERER,
                "bindings":[{"binding_name":"#second_visible", "binding_name_override":"#visible"}]}}
        ]}
    }).to_string());
    let mut engine = FormEngine::new(mini_carrier(), catalog, 2);
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(9, 9, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let files = [
        assets::HudTextureRole::HungerBackground,
        assets::HudTextureRole::HungerFull,
    ]
    .map(|role| (role.source_path().into(), png.clone()));
    engine.set_server_atlas(ServerAtlas::new(&files, None, 1), 3);
    engine
}

/// Paint actual bound custom controls and return their relative icon heights.
fn draw(
    engine: &FormEngine,
    screens: &mut HudScreens,
    player: &player_state::PlayerState,
    runtime: &UiRuntime,
    visible: [bool; 2],
) -> [Vec<f32>; 2] {
    let metrics = TextMetrics::for_viewport([1280, 720], DpiScale::new(1.0).unwrap(), None);
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut data = DataSource::default();
    data.set_global("#first_visible", json_ui::Scalar::Bool(visible[0]));
    data.set_global("#second_visible", json_ui::Scalar::Bool(visible[1]));
    screens
        .hunger_animation
        .begin(runtime.session_id(), engine.catalog());
    let paint = hud_layout::capture_hud_paint(
        player,
        runtime,
        &HudFrame::default(),
        None,
        &Default::default(),
    );
    let mut layouts = TextLayoutCache::new(32, 1024 * 1024);
    let mut nodes = Vec::new();
    let mut next = 1;
    let hunger_animation = std::cell::RefCell::new(&mut screens.hunger_animation);
    let advance_hunger = |key: &str| hunger_animation.borrow_mut().advance(key);
    engine
        .draw(
            ScreenArt {
                hud: Some(&paint),
                hunger_update: Some(&advance_hunger),
                ..Default::default()
            },
            EngineInputs {
                layouts: &mut layouts,
                font: &fixture_font(),
                metrics,
                solid_page: 0,
                safe_area: SafeArea::ZERO,
                content: [1280.0, 720.0],
                translate: &|_| None,
                language: [0; 3],
            },
            EngineOutput {
                nodes: &mut nodes,
                next: &mut next,
                overlay: &[],
            },
            |env, root| {
                screens.hud.render(
                    "hud.root",
                    engine.catalog(),
                    &Context::desktop(),
                    Arc::new(data),
                    (root, px, [0; 3]),
                    env,
                )
            },
        )
        .unwrap()
        .unwrap();
    let mut heights = [Vec::new(), Vec::new()];
    for node in nodes
        .iter()
        .filter(|node| matches!(node.visual(), ui::UiVisual::Sprite { .. }))
    {
        let y = node.bounds().min().y() / px;
        let row = usize::from(y > 45.0);
        heights[row].push(y - [30.0, 60.0][row]);
    }
    heights
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
fn hunger_controls_keep_independent_pulse_history() {
    let engine = engine();
    let mut screens = HudScreens::default();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        draw(&engine, &mut screens, &player, &runtime, [true, false]);
    }
    let rows = draw(&engine, &mut screens, &player, &runtime, [true, true]);
    assert!(rows[0].contains(&-1.0), "first control reaches its pulse");
    assert!(!rows[1].is_empty(), "second control is drawn");
    assert!(
        rows[1].iter().all(|y| *y == 0.0),
        "newly shown hunger control starts neutral"
    );
}

#[test]
fn repeated_zero_tick_food_updates_do_not_restart_hunger_motion() {
    let engine = engine();
    let mut screens = HudScreens::default();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    for update in 1..=110 {
        food_update(&mut player, &mut runtime, update);
        let rows = draw(&engine, &mut screens, &player, &runtime, [true, false]);
        assert!(!rows[0].is_empty());
        assert!(rows[0].iter().all(|y| [-1.0, 0.0].contains(y)));
        if update % 55 == 0 {
            assert!(rows[0].contains(&-1.0));
        } else {
            assert!(
                rows[0].iter().all(|y| *y == 0.0),
                "packet restarted pulse at {update}"
            );
        }
        for layers in rows[0].chunks_exact(2).take(9) {
            assert_eq!(layers[0], layers[1]);
        }
    }
}

#[test]
fn a_new_session_starts_with_a_neutral_hunger_row() {
    let engine = engine();
    let mut screens = HudScreens::default();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        draw(&engine, &mut screens, &player, &runtime, [true, false]);
    }
    let mut player = player_state::PlayerState::new(2);
    let mut runtime = UiRuntime::new(2);
    food_update(&mut player, &mut runtime, 1);
    let rows = draw(&engine, &mut screens, &player, &runtime, [true, false]);
    assert!(!rows[0].is_empty());
    assert!(rows[0].iter().all(|y| *y == 0.0));
}

#[test]
fn a_pack_hidden_hunger_renderer_does_not_advance_its_pulse() {
    let engine = engine();
    let mut screens = HudScreens::default();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        draw(&engine, &mut screens, &player, &runtime, [true, false]);
    }
    for _ in 0..100 {
        let rows = draw(&engine, &mut screens, &player, &runtime, [false, true]);
        assert!(rows[0].is_empty());
        assert!(!rows[1].is_empty(), "another hunger control stays shown");
    }
    let rows = draw(&engine, &mut screens, &player, &runtime, [true, true]);
    assert!(
        rows[0].contains(&-1.0),
        "hidden control resumes its pending pulse"
    );
}

#[test]
fn a_new_pack_catalog_starts_with_a_neutral_hunger_row() {
    let first_engine = engine();
    let mut screens = HudScreens::default();
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    food_update(&mut player, &mut runtime, 1);
    for _ in 0..54 {
        draw(
            &first_engine,
            &mut screens,
            &player,
            &runtime,
            [true, false],
        );
    }
    let replacement_engine = engine();
    let rows = draw(
        &replacement_engine,
        &mut screens,
        &player,
        &runtime,
        [true, false],
    );
    assert!(!rows[0].is_empty());
    assert!(rows[0].iter().all(|y| *y == 0.0));
}
