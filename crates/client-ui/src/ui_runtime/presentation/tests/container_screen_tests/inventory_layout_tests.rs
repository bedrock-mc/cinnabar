use super::*;
use crate::ui_runtime::{SequencedUiEvent, inventory_drag::PointerAction};
use ui_screens::container_actions::Widget;

#[test]
fn negative_inventory_layout_draws_as_hidden_recipe_book() {
    use crate::ui_runtime::presentation::forms::{pack_harness, snapshot};
    let Some(mut presentation) = engine_presentation_with(pack_harness::font()) else {
        eprintln!(
            "skipping negative_inventory_layout_draws_as_hidden_recipe_book: missing local UI carrier; make assets"
        );
        return;
    };
    let mut hidden = None;
    for layout in [
        protocol::InventoryLayout::InventoryOnly,
        protocol::InventoryLayout::Unknown(-1),
    ] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = session(&mut player);
        player
            .facts
            .publish_player_game_mode(protocol::PlayerGameMode::Survival);
        player
            .inventory
            .set_inventory_options(protocol::InventoryOptions {
                inventory_layout: layout,
                ..Default::default()
            });
        runtime.toggle_inventory(&mut player, None);
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
        assert!(!crate::ui_runtime::presentation::forms::recipe_book_shown(
            &player, &runtime
        ));
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            layout
        );
        let raster = snapshot::rasterize(&input);
        if let Some(hidden) = &hidden {
            snapshot::write(&input, "inventory-negative-layout");
            assert!(
                raster == *hidden,
                "a hidden book must draw the inventory-only screen"
            );
        } else {
            hidden = Some(raster);
        }
    }
}

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

/// The full creative inventory over a grouped 300-item catalog, explicitly showing player cells.
pub(super) fn creative_with_player_cells(
    player_runtime: &mut player_state::PlayerState,
) -> UiRuntime {
    let mut runtime = creative_with(player_runtime, 300);
    runtime
        .change_inventory_options(player_runtime, false, |controller| {
            controller.set_layout(protocol::InventoryLayout::RecipeBook)
        })
        .unwrap();
    runtime
}

// Creative's wide list drops the player inventory for the catalog and keeps
// the hotbar beneath it; its toggles pick each layout.
#[test]
fn creative_wide_layout_keeps_only_the_hotbar_under_the_catalog() {
    let mut player_runtime = player_state::PlayerState::new(1);

    use ui_screens::container_actions::Widget as W;
    let Some(mut presentation) =
        engine_presentation_with(super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping creative_wide_layout_keeps_only_the_hotbar_under_the_catalog: fixture unavailable; requires installed local carriers (make assets)"
        );
        return;
    };
    let mut runtime = creative_with(&mut player_runtime, 300);
    let stored = player_runtime.inventory.inventory_options();
    assert_eq!(stored, protocol::InventoryOptions::default());
    assert_eq!(
        runtime
            .inventory_controller(&player_runtime)
            .unwrap()
            .layout(),
        protocol::InventoryLayout::RecipeBookOnly
    );
    assert!(runtime.take_client_packet().is_none());
    let dpi = DpiScale::new(1.0).unwrap();
    for now in [0, 500] {
        presentation
            .build(&player_runtime, &runtime, now, [1280, 720], dpi)
            .unwrap();
    }
    let input = presentation
        .build(&player_runtime, &runtime, 5_000, [1280, 720], dpi)
        .unwrap();
    super::super::forms::snapshot::write(&input, "container-creative_wide");
    let frame = presentation.engine_container_frame().unwrap();
    let reached: Vec<InventoryCellHit> = frame
        .hits
        .iter()
        .filter_map(|region| {
            let center = [
                (region.rect.x + region.rect.w / 2.0) as f32,
                (region.rect.y + region.rect.h / 2.0) as f32,
            ];
            presentation.engine_container_hit(center)
        })
        .collect();
    for hit in [
        InventoryCellHit::RecipeBook(0),
        InventoryCellHit::Player(0),
        InventoryCellHit::Player(8),
        InventoryCellHit::Widget(W::InventoryLayout(2)),
    ] {
        assert!(reached.contains(&hit), "{hit:?} unreachable");
    }
    for slot in 0..protocol::PLAYER_INVENTORY_SLOTS {
        assert_eq!(
            reached.contains(&InventoryCellHit::Player(slot)),
            slot < protocol::HOTBAR_SLOT_COUNT,
            "default creative layout: player slot {slot}"
        );
    }
    assert_eq!(player_runtime.inventory.inventory_options(), stored);
    assert!(runtime.take_client_packet().is_none());
}
