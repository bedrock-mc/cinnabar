use super::*;
use crate::ui_runtime::inventory_drag::PointerAction;
use protocol::{InventoryLayout, InventoryOptions, InventoryRightTab, InventoryTab};

#[test]
fn creative_fallback_search_retains_focus_with_hidden_recipe_layouts() {
    use crate::ui_runtime::presentation::{
        forms::snapshot, inventory_pointer::InventoryScreen, screens,
    };
    use ui_screens::container_actions::SEARCH_TAB;

    for layout in [
        InventoryLayout::InventoryOnly,
        InventoryLayout::Unknown(-1),
        InventoryLayout::Unknown(i32::MIN),
    ] {
        for captured in [true, false] {
            let mut player = player_state::PlayerState::new(1);
            let mut runtime = creative_with(&mut player, 40);
            let options = InventoryOptions {
                inventory_layout: layout,
                ..InventoryOptions::default()
            };
            runtime.close_inventory(&mut player);
            player.inventory.set_inventory_options(options);
            runtime.toggle_inventory(&mut player, None);
            if !captured {
                runtime.screen_state_mut().inventory = None;
            }
            let mut presentation = UiPresentationRuntime::with_hud(
                ui_engine::test_support::fixture_font(),
                crate::test_support::fixture_hud(),
            )
            .unwrap();
            let size = [1280, 720];
            let dpi = DpiScale::new(1.0).unwrap();
            presentation.build(&player, &runtime, 0, size, dpi).unwrap();
            assert!(presentation.engine_container_frame().is_none());
            assert!(!super::super::super::forms::recipe_book_shown(
                &player, &runtime
            ));
            let screen = InventoryScreen::of_runtime(&player, &runtime);
            assert_eq!(screen, InventoryScreen::Creative);
            let gui_size = presentation
                .inventory_gui_point(
                    ui::UiPoint::new(size[0] as f32, size[1] as f32).unwrap(),
                    size,
                    1.0,
                )
                .unwrap();
            let origin = screens::panel_origin(screen, gui_size);
            let tab = screens::tab_origin(SEARCH_TAB);
            let hit = presentation
                .inventory_cell_hit(
                    [origin[0] + tab[0] + 1.0, origin[1] + tab[1] + 1.0],
                    size,
                    1.0,
                    screen,
                )
                .expect("fallback Search tab is reachable");
            assert_eq!(hit, InventoryCellHit::CreativeSearch);
            runtime.perform_pointer_action(&mut player, PointerAction::Click(hit));
            assert!(runtime.screen_state().text_focused());
            runtime.observe_inventory_search_focus(
                &player,
                presentation.engine_container_frame().is_some(),
            );
            runtime.screen_state_mut().type_text("iron");
            let input = presentation
                .build(&player, &runtime, 500, size, dpi)
                .unwrap();
            snapshot::write(
                &input,
                &format!("creative-fallback-search-{}-{captured}", layout.index()),
            );
            assert!(
                runtime.screen_state().text_focused(),
                "visible fallback search retains focus for {layout:?}, captured={captured}"
            );
            assert_eq!(runtime.screen_state().search, "iron");
            runtime.observe_inventory_search_focus(
                &player,
                presentation.engine_container_frame().is_some(),
            );
            runtime.screen_state_mut().backspace_text();
            assert_eq!(runtime.screen_state().search, "iro");
            runtime.observe_inventory_search_focus(&player, true);
            assert!(
                !runtime.screen_state().text_focused(),
                "the engine renderer releases hidden search for {layout:?}"
            );
            assert_eq!(player.inventory.inventory_options(), options);
            assert!(runtime.take_client_packet().is_none());
        }
    }
}

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
    runtime.observe_inventory_search_focus(player, presentation.engine_container_frame().is_some());
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
        runtime.observe_inventory_search_focus(
            &player,
            presentation.engine_container_frame().is_some(),
        );
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
        runtime.observe_inventory_search_focus(
            &player,
            presentation.engine_container_frame().is_some(),
        );
        assert!(!runtime.screen_state().text_focused());
    }
}
