use super::*;
use crate::ui_runtime::inventory_drag::PointerAction;
use protocol::{InventoryLayout, InventoryOptions, InventoryRightTab, InventoryTab};

/// Clicks the drawn search box and keeps it focused through the next pointer frame.
fn edit_search(
    presentation: &mut UiPresentationRuntime,
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
) {
    presentation
        .build(player, runtime, 0, [1280, 720], DpiScale::new(1.0).unwrap())
        .unwrap();
    let field = presentation
        .engine_container_frame()
        .unwrap()
        .hits
        .iter()
        .find(|hit| hit.kind == json_ui::HitKind::EditBox)
        .expect("visible recipe search box");
    let hit = presentation
        .engine_container_hit([
            (field.rect.x + field.rect.w / 2.0) as f32,
            (field.rect.y + field.rect.h / 2.0) as f32,
        ])
        .expect("search box is reachable");
    assert_eq!(hit, InventoryCellHit::CreativeSearch);
    runtime.screen_state_mut().creative_row = 4;
    runtime
        .screen_state_mut()
        .container_scroll
        .insert("recipes".into(), 24.0);
    runtime.perform_pointer_action(player, PointerAction::Click(hit));
    assert!(
        runtime.screen_state().text_focused(),
        "click focuses search"
    );
    assert_eq!(runtime.screen_state().creative_row, 4);
    assert_eq!(runtime.screen_state().container_scroll["recipes"], 24.0);
    runtime.observe_inventory_search_focus(player);
    assert!(
        runtime.screen_state().text_focused(),
        "the next pointer frame retains visible search focus"
    );
    runtime.screen_state_mut().type_text("iron");
    runtime.screen_state_mut().backspace_text();
    assert_eq!(runtime.screen_state().search, "iro");
    assert_eq!(runtime.screen_state().creative_row, 0);
    assert!(runtime.screen_state().container_scroll.is_empty());
}

#[test]
fn furnace_search_click_retains_focus_without_changing_inventory_options() {
    let Some(mut presentation) =
        engine_presentation_with(super::super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping furnace_search_click_retains_focus_without_changing_inventory_options: missing local UI carrier; make assets"
        );
        return;
    };
    for window in [
        protocol::WINDOW_TYPE_FURNACE,
        protocol::WINDOW_TYPE_BLAST_FURNACE,
        protocol::WINDOW_TYPE_SMOKER,
    ] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = opened(&mut player, window, 3);
        let inventory = InventoryOptions {
            left_tab: InventoryTab::Nature,
            right_tab: InventoryRightTab::Armor,
            filtering: true,
            inventory_layout: InventoryLayout::InventoryOnly,
            crafting_layout: InventoryLayout::Unknown(83),
        };
        player.inventory.set_inventory_options(inventory);
        let kind =
            protocol::FurnaceKind::from_window(player.inventory.ledger().window_kind().unwrap())
                .unwrap();
        let stored = player.inventory.furnace_options(kind);
        let shown = runtime.furnace_options(&player);
        edit_search(&mut presentation, &mut player, &mut runtime);
        assert_eq!(player.inventory.inventory_options(), inventory);
        assert!(runtime.screen_state().inventory.is_none());
        assert_eq!(player.inventory.furnace_options(kind), stored);
        assert_eq!(runtime.furnace_options(&player), shown);
        assert!(runtime.take_client_packet().is_none());
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::Widget(
                ui_screens::container_actions::Widget::InventoryLayout(1),
            )),
        );
        runtime.observe_inventory_search_focus(&player);
        assert!(!runtime.screen_state().text_focused());
    }
}

#[test]
fn inventory_search_click_retains_focus_in_personal_and_workbench_screens() {
    let Some(mut presentation) =
        engine_presentation_with(super::super::super::forms::pack_harness::font())
    else {
        eprintln!(
            "skipping inventory_search_click_retains_focus_in_personal_and_workbench_screens: missing local UI carrier; make assets"
        );
        return;
    };
    for (creative, workbench) in [(false, false), (false, true), (true, false)] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = if creative {
            creative_with(&mut player, 40)
        } else if workbench {
            opened(&mut player, protocol::WINDOW_TYPE_WORKBENCH, 0)
        } else {
            personal(&mut player)
        };
        let stored = player.inventory.inventory_options();
        let shown = runtime.inventory_controller(&player);
        edit_search(&mut presentation, &mut player, &mut runtime);
        assert_eq!(player.inventory.inventory_options(), stored);
        assert_eq!(runtime.inventory_controller(&player), shown);
        assert!(runtime.take_client_packet().is_none());
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::Widget(
                ui_screens::container_actions::Widget::InventoryLayout(1),
            )),
        );
        runtime.observe_inventory_search_focus(&player);
        assert!(!runtime.screen_state().text_focused());
    }
}
