//! Surfaces outside the engine HUD: the inventory's player preview, the
//! effect-id gate, and the held tab player list.

use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent, NetworkItemStack,
};

use super::{fixture_font, fixture_hud};
use crate::ui_runtime::UiRuntime;
use crate::ui_runtime::presentation::UiPresentationRuntime;

#[test]
fn player_preview_only_renders_in_the_personal_inventory() {
    let mut player_runtime = player_state::PlayerState::new(1);

    let mut presentation = UiPresentationRuntime::with_hud(fixture_font(), fixture_hud()).unwrap();
    presentation.set_player_preview_skin(None, Default::default());
    let preview = presentation
        .player_preview_icon()
        .expect("the fixture texture array has room for the player preview");
    presentation.hud_frame_mut().player_preview = Some(preview);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, protocol::InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();

    let gameplay = build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(
        gameplay
            .batches
            .iter()
            .all(|batch| batch.texture_page != u32::from(preview.page)),
        "ordinary gameplay has no persistent player preview"
    );

    runtime.toggle_inventory(&mut player_runtime);
    let personal_inventory = build(&player_runtime, &mut presentation, &runtime, 0);
    assert!(
        personal_inventory
            .batches
            .iter()
            .any(|batch| batch.texture_page == u32::from(preview.page)),
        "the personal inventory retains its player preview"
    );

    for (window_id, slot_count) in [(7, 27), (8, 54)] {
        let mut player_runtime = player_state::PlayerState::new(1);
        let mut storage_runtime = UiRuntime::new(1);
        storage_runtime
            .enqueue_inventory_event(
                &mut player_runtime,
                1,
                1,
                InventoryEvent::Open(ContainerOpenEvent {
                    container: ContainerIdentity {
                        window_id: Some(window_id),
                        slot_type: None,
                        dynamic_id: None,
                    },
                    window_type: 0,
                    position: [0; 3],
                    runtime_entity_id: 0,
                }),
            )
            .unwrap();
        storage_runtime.drain_pending_inventory(&mut player_runtime);
        let awaiting_content = build(&player_runtime, &mut presentation, &storage_runtime, 0);
        assert!(
            awaiting_content
                .batches
                .iter()
                .all(|batch| batch.texture_page != u32::from(preview.page)),
            "an opened storage window waiting for content stays preview-free"
        );

        storage_runtime
            .enqueue_inventory_event(
                &mut player_runtime,
                1,
                2,
                InventoryEvent::Content(InventoryContentEvent {
                    container: ContainerIdentity {
                        window_id: Some(window_id),
                        slot_type: Some(7),
                        dynamic_id: None,
                    },
                    slots: vec![NetworkItemStack::empty(); slot_count].into(),
                    storage_item: NetworkItemStack::empty(),
                }),
            )
            .unwrap();
        storage_runtime.drain_pending_inventory(&mut player_runtime);
        let storage = build(&player_runtime, &mut presentation, &storage_runtime, 0);
        assert!(
            storage
                .batches
                .iter()
                .all(|batch| batch.texture_page != u32::from(preview.page)),
            "supported storage screens stay preview-free"
        );
    }
}

fn build(
    player_runtime: &player_state::PlayerState,
    presentation: &mut UiPresentationRuntime,
    runtime: &UiRuntime,
    now_millis: u64,
) -> render_model::UiRenderInput {
    presentation
        .build(
            player_runtime,
            runtime,
            now_millis,
            [1280, 720],
            ui::DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn the_renderable_effect_id_gate_matches_the_pinned_icon_table_exactly() {
    use crate::ui_runtime::gameplay_hud::is_renderable_effect_id;
    use ui::native_hud::effect_icon_role;
    for effect_id in -8..=64 {
        assert_eq!(
            is_renderable_effect_id(effect_id),
            effect_icon_role(effect_id).is_some(),
            "state gate and pinned icon table agree on effect id {effect_id}"
        );
    }
}
