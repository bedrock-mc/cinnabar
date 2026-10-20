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

#[test]
fn trade_previews_reach_the_rich_tooltip_with_complete_item_data() {
    use ui_screens::container_actions::{InventoryCellHit, Widget};
    let mut player = player_state::PlayerState::new(1);
    let mut runtime = UiRuntime::new(1);
    let ledger = player.inventory.ledger_mut();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        64,
        0,
    ));
    let data = protocol::edit_item_extra(
        &[0; 10],
        &protocol::ItemNbtEdit {
            custom_name: Some(Some("Named offer".into())),
            enchantments: Some(vec![(0, 3)]),
            ..Default::default()
        },
    )
    .unwrap();
    let output = protocol::TradeItem {
        identifier: Arc::from("minecraft:apple"),
        metadata: 0,
        count: 1,
        block_descriptor: None,
        extra_data: Arc::clone(&data),
    };
    ledger.apply(&InventoryEvent::Trade(TradeEvent {
        container: ContainerIdentity::window(7),
        actor_unique_id: 90,
        title: Arc::from("Trader"),
        modern: true,
        tier: 0,
        offers: Arc::from([protocol::TradeOffer {
            network_id: 91,
            inputs: [Some(output.clone()), None],
            output,
            tier: 0,
            uses: 0,
            max_uses: 16,
        }]),
    }));
    for widget in [
        Widget::TradeOffer {
            tier: 0,
            row: 0,
            item: Some(2),
        },
        Widget::TradePreview(2),
    ] {
        let hit = InventoryCellHit::Widget(widget);
        assert!(hit.is_item_cell());
        runtime.screen_state_mut().hover = Some(hit);
        let (stack, _) = presentation::publish::hovered_stack(&player, &runtime).unwrap();
        assert_eq!(stack.extra_data, data);
        let details = protocol::item_anvil_data(&stack.extra_data).unwrap();
        assert_eq!(details.custom_name.as_deref(), Some("Named offer"));
        assert_eq!(details.enchantments, [(0, 3)]);
    }
}
