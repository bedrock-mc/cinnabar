use super::*;
use protocol::{ContainerIdentity, InventoryAuthority, InventoryEvent, TradeEvent};

#[test]
fn trade_offers_open_the_server_selected_screen_without_a_container_open() {
    for modern in [false, true] {
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = UiRuntime::new(1);
        for (sequence, event) in [
            InventoryEvent::Authority(InventoryAuthority::Server),
            InventoryEvent::Trade(TradeEvent {
                container: ContainerIdentity::window(7),
                actor_unique_id: 90,
                title: Arc::from("Trader"),
                modern,
                tier: 0,
                offers: Arc::from([]),
            }),
        ]
        .into_iter()
        .enumerate()
        {
            runtime
                .enqueue_inventory_event(&mut player, 1, sequence as u64 + 1, event)
                .unwrap();
        }
        runtime.drain_pending_inventory(&mut player);
        assert!(runtime.inventory_open());
        assert_eq!(
            presentation::forms::container_screen_reference(&player, &runtime),
            Some(if modern {
                json_ui::vanilla::Reference::Trade2TradeScreen.as_str()
            } else {
                json_ui::vanilla::Reference::TradeTradeScreen.as_str()
            })
        );
        runtime.toggle_inventory(&mut player, None);
        assert!(!runtime.inventory_open());
        assert!(!player.inventory.ledger().storage_inventory_desired_open());
    }
}
