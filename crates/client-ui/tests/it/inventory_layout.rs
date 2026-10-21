use client_ui::ui_runtime::{
    UiRuntime, inventory_drag::PointerAction, presentation::forms::recipe_book_shown,
};
use player_state::PlayerState;
use protocol::{
    GameModeEvent, GameModeUpdate, HudEvent, InventoryLayout, InventoryOptions, PlayerGameMode,
    UiEvent,
};
use ui_screens::container_actions::{InventoryCellHit, Widget};

use super::fixtures::envelope;

/// Applies the layout radio action used by the native inventory screen.
fn choose_layout(player: &mut PlayerState, runtime: &mut UiRuntime, shown: bool) {
    runtime.perform_pointer_action(
        player,
        PointerAction::Click(InventoryCellHit::Widget(Widget::InventoryLayout(
            if shown { 2 } else { 1 },
        ))),
    );
}

#[test]
fn untouched_classic_layout_shows_the_recipe_book_and_book_toggle_records_a_choice() {
    for mode in [PlayerGameMode::Survival, PlayerGameMode::Creative] {
        let mut player = PlayerState::new(1);
        player.facts.publish_player_game_mode(mode);
        let mut runtime = UiRuntime::new(1);
        assert!(recipe_book_shown(&player, &runtime));
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            if mode == PlayerGameMode::Creative {
                InventoryLayout::RecipeBookOnly
            } else {
                InventoryLayout::RecipeBook
            }
        );
        assert_eq!(
            player.inventory.inventory_options(),
            InventoryOptions::default()
        );
        runtime.perform_pointer_action(
            &mut player,
            PointerAction::Click(InventoryCellHit::Widget(Widget::BookToggle)),
        );
        assert!(!recipe_book_shown(&player, &runtime));
        assert_eq!(
            player.inventory.inventory_options().inventory_layout,
            InventoryLayout::InventoryOnly
        );
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Spectator);
        assert!(!recipe_book_shown(&player, &runtime));
    }
}

#[test]
fn selected_inventory_layout_survives_death_and_game_mode_changes() {
    for initial in [PlayerGameMode::Survival, PlayerGameMode::Creative] {
        for shown in [false, true] {
            let mut player = PlayerState::new(1);
            player.facts.publish_player_game_mode(initial);
            let mut runtime = UiRuntime::new(1);
            choose_layout(&mut player, &mut runtime, shown);
            assert_eq!(recipe_book_shown(&player, &runtime), shown);
            runtime
                .apply(
                    &mut player,
                    envelope(1, 1, UiEvent::Hud(HudEvent::Health { health: 0 })),
                )
                .unwrap();
            runtime.close_inventory(&mut player);
            for (index, mode) in [
                PlayerGameMode::Spectator,
                PlayerGameMode::Survival,
                PlayerGameMode::Creative,
            ]
            .into_iter()
            .enumerate()
            {
                runtime
                    .apply(
                        &mut player,
                        envelope(
                            1,
                            index as u64 + 2,
                            UiEvent::GameMode(GameModeEvent {
                                update: GameModeUpdate::Explicit(mode),
                            }),
                        ),
                    )
                    .unwrap();
                assert_eq!(
                    recipe_book_shown(&player, &runtime),
                    shown,
                    "{initial:?} -> {mode:?}"
                );
            }
            runtime
                .apply(
                    &mut player,
                    envelope(1, 5, UiEvent::Hud(HudEvent::Health { health: 20 })),
                )
                .unwrap();
            assert_eq!(recipe_book_shown(&player, &runtime), shown);
        }
    }
}

#[test]
fn session_replacement_resets_inventory_options_and_input_focus() {
    for shown in [false, true] {
        let mut player = PlayerState::new(1);
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        let mut runtime = UiRuntime::new(1);
        choose_layout(&mut player, &mut runtime, shown);
        runtime.screen_state_mut().search = "old search".into();
        runtime.screen_state_mut().search_focused = true;
        player.begin_session(2);
        runtime.begin_session(2);
        player
            .facts
            .publish_player_game_mode(PlayerGameMode::Creative);
        assert!(recipe_book_shown(&player, &runtime));
        assert_eq!(
            runtime.inventory_controller(&player).unwrap().layout(),
            InventoryLayout::RecipeBookOnly
        );
        assert_eq!(
            player.inventory.inventory_options(),
            InventoryOptions::default()
        );
        assert!(!runtime.screen_state().text_focused());
        assert!(runtime.screen_state().search.is_empty());
    }
}
