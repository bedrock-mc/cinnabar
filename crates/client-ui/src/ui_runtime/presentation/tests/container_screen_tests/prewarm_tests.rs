use super::*;

use crate::ui_runtime::presentation::forms::ServerUiPack;
use serde_json::json;

/// Installs an original inventory definition over the in-memory engine carrier.
fn inventory_fixture(
    definition: serde_json::Value,
) -> (UiPresentationRuntime, UiRuntime, player_state::PlayerState) {
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player);
    runtime.set_server_ui(Some(Arc::new(ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".into(),
                br#"{"ui_defs":["ui/crafting.json"]}"#.to_vec(),
            ),
            (
                "ui/crafting.json".into(),
                serde_json::to_vec(&definition).unwrap(),
            ),
        ]],
        ..Default::default()
    })));
    let mut presentation = crate::test_support::mini_engine_presentation();
    presentation.gui_scale_preference = Some(1);
    (presentation, runtime, player)
}

/// Builds at the caller's animation time with a fixed GUI scale and viewport.
fn draw(
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    player: &player_state::PlayerState,
    now: u64,
) -> UiRenderInput {
    presentation
        .build(
            player,
            runtime,
            now,
            [320, 240],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

/// Publishes one occupied hotbar cell; names remain caller-controlled frame data.
fn populate(runtime: &mut UiRuntime, player: &mut player_state::PlayerState) {
    runtime
        .enqueue_inventory_event(
            player,
            1,
            1,
            InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity::window(0),
                slots: vec![NetworkItemStack {
                    network_id: 1,
                    count: 1,
                    stack_network_id: 1,
                    ..NetworkItemStack::empty()
                }]
                .into(),
                storage_item: NetworkItemStack::empty(),
            }),
        )
        .unwrap();
    runtime.drain_pending_inventory(player);
}

/// A server label that captures its hotbar cell's hover name only once.
fn once_name_screen() -> serde_json::Value {
    json!({"namespace":"crafting", "inventory_screen": {
        "type":"screen", "controls":[{"slot":{
            "type":"panel", "size":[100,12], "collection_name":"hotbar_items",
            "controls":[{"name":{
                "type":"label", "size":[100,12], "text":"#text", "collection_index":0,
                "bindings":[{
                    "binding_type":"collection", "binding_collection_name":"hotbar_items",
                    "binding_name":"#hover_text", "binding_name_override":"#text",
                    "binding_condition":"once"
                }]
            }}]
        }}]
    }})
}

#[test]
fn prewarmed_once_binding_reads_the_name_at_first_open() {
    let (mut warm, mut runtime, mut player) = inventory_fixture(once_name_screen());
    let (mut cold, _, _) = inventory_fixture(once_name_screen());
    populate(&mut runtime, &mut player);
    let closed = draw(&mut warm, &runtime, &player, 0);
    assert_eq!(warm.engine_container_resolves(), 1);
    assert!(warm.engine_container_frame().is_none());
    assert!(warm.hud_frame().item_names.is_empty());

    runtime.toggle_inventory(&mut player, None);
    for presentation in [&mut warm, &mut cold] {
        presentation
            .hud_frame_mut()
            .item_names
            .insert((1, 0), Arc::from("200"));
    }
    let expected = draw(&mut cold, &runtime, &player, 100);
    let opened = draw(&mut warm, &runtime, &player, 100);
    assert!(cold.engine_container_frame().is_some());
    assert!(warm.engine_container_frame().is_some());
    assert!(
        !expected.vertices.is_empty(),
        "the once-bound name must draw"
    );
    assert_ne!(opened.vertices, closed.vertices);
    assert_eq!(
        opened.vertices, expected.vertices,
        "prewarm must not capture an empty name"
    );
    assert_eq!(
        warm.engine_container_resolves(),
        1,
        "opening reuses the resolved screen"
    );

    warm.hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("00"));
    let refreshed = draw(&mut warm, &runtime, &player, 200);
    assert_eq!(
        refreshed.vertices, opened.vertices,
        "the real open must retain its once binding"
    );
}

#[test]
fn closing_an_inventory_opened_without_prewarm_keeps_its_once_binding() {
    let (mut presentation, mut runtime, mut player) = inventory_fixture(once_name_screen());
    populate(&mut runtime, &mut player);
    runtime.toggle_inventory(&mut player, None);
    presentation
        .hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("200"));
    let first = draw(&mut presentation, &runtime, &player, 0);
    assert!(!first.vertices.is_empty());
    runtime.toggle_inventory(&mut player, None);
    presentation.hud_frame_mut().item_names.clear();
    draw(&mut presentation, &runtime, &player, 100);
    runtime.toggle_inventory(&mut player, None);
    presentation
        .hud_frame_mut()
        .item_names
        .insert((1, 0), Arc::from("00"));
    let reopened = draw(&mut presentation, &runtime, &player, 200);
    assert_eq!(
        reopened.vertices, first.vertices,
        "closing must not reset live once bindings"
    );
}

#[test]
fn prewarmed_inventory_animation_starts_at_the_real_open() {
    for wait_until_rendered in [false, true] {
        let definition = json!({"namespace":"crafting",
            "opening":{"anim_type":"alpha", "duration":1.0, "from":0.25, "to":1.0,
                "wait_until_rendered_to_play":wait_until_rendered},
            "inventory_screen":{"type":"screen", "controls":[{"animated":{
                "type":"label", "size":[100,12], "text":"200", "alpha":"@crafting.opening"
            }}]}
        });
        let (mut warm, mut runtime, mut player) = inventory_fixture(definition.clone());
        let (mut cold, _, _) = inventory_fixture(definition);
        draw(&mut warm, &runtime, &player, 0);
        assert_eq!(warm.engine_container_resolves(), 1);
        assert!(warm.engine_container_frame().is_none());
        runtime.toggle_inventory(&mut player, None);
        let mut first = None;
        for now in [100, 300, 600] {
            let expected = draw(&mut cold, &runtime, &player, now);
            let opened = draw(&mut warm, &runtime, &player, now);
            assert!(!expected.vertices.is_empty());
            assert_eq!(
                opened.vertices, expected.vertices,
                "animation phase at {now}; wait_until_rendered={wait_until_rendered}"
            );
            if let Some(first) = &first {
                assert_ne!(
                    &opened.vertices, first,
                    "the opening animation must advance"
                );
            } else {
                first = Some(opened.vertices);
            }
        }
        assert_eq!(warm.engine_container_resolves(), 1);
    }
}
