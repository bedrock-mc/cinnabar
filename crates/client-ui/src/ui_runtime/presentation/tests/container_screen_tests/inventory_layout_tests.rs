use super::*;
use crate::ui_runtime::{SequencedUiEvent, inventory_drag::PointerAction};
use ui_screens::container_actions::Widget;

#[test]
fn inventory_only_layout_survives_respawn_in_the_rendered_screen() {
    let Some(mut presentation) =
        engine_presentation_with(super::super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping inventory_only_layout_survives_respawn_in_the_rendered_screen: missing local UI carrier; make assets"
        );
        return;
    };
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = session(&mut player);
    player
        .facts
        .publish_player_game_mode(protocol::PlayerGameMode::Survival);
    runtime.perform_pointer_action(
        &mut player,
        PointerAction::Click(InventoryCellHit::Widget(Widget::InventoryLayout(1))),
    );
    for (index, event) in [
        protocol::UiEvent::Hud(protocol::HudEvent::Health { health: 0 }),
        protocol::UiEvent::GameMode(protocol::GameModeEvent {
            update: protocol::GameModeUpdate::Explicit(protocol::PlayerGameMode::Creative),
        }),
        protocol::UiEvent::Hud(protocol::HudEvent::Health { health: 20 }),
    ]
    .into_iter()
    .enumerate()
    {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: index as u64 + 1,
                    local_millis: index as u64,
                    server_tick: None,
                    event,
                },
            )
            .unwrap();
    }
    runtime.toggle_inventory(&mut player, None);
    assert!(runtime.inventory_open());
    for now in [0, 500] {
        presentation
            .build(
                &player,
                &runtime,
                now,
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
    }
    let input = presentation
        .build(
            &player,
            &runtime,
            5_000,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
    super::super::super::forms::snapshot::write(&input, "inventory-layout-after-respawn");
    assert!(!crate::ui_runtime::presentation::forms::recipe_book_shown(
        &player, &runtime
    ));
    let frame = presentation
        .engine_container_frame()
        .expect("native inventory screen");
    assert!(
        !frame
            .hits
            .iter()
            .any(|hit| hit.collection.as_deref() == Some("recipe_book"))
    );
}
