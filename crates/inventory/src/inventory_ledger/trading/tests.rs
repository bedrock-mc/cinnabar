use super::*;
use protocol::{
    ContainerIdentity, InventoryAuthority, InventoryEvent, SlotIdentity, StackRequestAction,
    StackRequestContainer, StackResponse, StackResponseStatus,
};

/// Creates original offers against a negotiated test item with server-owned input cells.
fn ledger(modern: bool) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        64,
        0,
    ));
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
        slots: vec![NetworkItemStack::empty(); usize::from(protocol::PLAYER_INVENTORY_SLOTS)]
            .into(),
        storage_item: NetworkItemStack::empty(),
    }));
    let item = |count| TradeItem {
        identifier: Arc::from("minecraft:apple"),
        metadata: 0,
        count,
        block_descriptor: None,
        extra_data: Arc::from([0; 10]),
    };
    ledger.apply(&InventoryEvent::Trade(TradeEvent {
        container: ContainerIdentity::window(7),
        actor_unique_id: 90,
        title: Arc::from("Trader"),
        modern,
        tier: 0,
        offers: Arc::from([TradeOffer {
            network_id: 91,
            inputs: [Some(item(2)), Some(item(1))],
            output: item(1),
            tier: 0,
            uses: 0,
            max_uses: 2,
        }]),
    }));
    let slots = if modern {
        protocol::MODERN_TRADE_SLOTS
    } else {
        protocol::LEGACY_TRADE_SLOTS
    };
    for (index, slot) in slots.iter().enumerate() {
        ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: u16::from(*slot),
            },
            stack: NetworkItemStack {
                network_id: 6,
                stack_network_id: index as i32 + 20,
                count: 5,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
    }
    ledger
}

#[test]
fn trading_opens_from_its_offers_and_uses_each_screens_named_inputs() {
    for modern in [false, true] {
        let mut ledger = ledger(modern);
        assert_eq!(ledger.window_kind(), Some(WindowKind::Trade { modern }));
        assert!(ledger.storage_inventory_desired_open());
        assert_eq!(ledger.window_actor(), Some(90));
        let request = ledger.begin_trade().unwrap();
        assert!(matches!(
            ledger.queue.back().unwrap().actions[0],
            StackRequestAction::CraftRecipe {
                recipe_network_id: 91,
                crafts: 1
            }
        ));
        let expected = if modern {
            [
                protocol_ids::ContainerName::TradeTwoIngredientOne,
                protocol_ids::ContainerName::TradeTwoIngredientTwo,
            ]
        } else {
            [
                protocol_ids::ContainerName::TradeIngredientOne,
                protocol_ids::ContainerName::TradeIngredientTwo,
            ]
        };
        let consumed: Vec<_> = ledger
            .queue
            .back()
            .unwrap()
            .actions
            .iter()
            .filter_map(|action| match action {
                StackRequestAction::Consume { amount, source } => Some((*amount, source.container)),
                _ => None,
            })
            .collect();
        assert_eq!(
            consumed,
            vec![
                (
                    2,
                    StackRequestContainer::OpenWindow {
                        name: expected[0].wire_value(),
                        dynamic_id: None
                    }
                ),
                (
                    1,
                    StackRequestContainer::OpenWindow {
                        name: expected[1].wire_value(),
                        dynamic_id: None
                    }
                )
            ]
        );
        assert_eq!(ledger.cursor_stack().unwrap().count, 1);
        let slots = if modern {
            protocol::MODERN_TRADE_SLOTS
        } else {
            protocol::LEGACY_TRADE_SLOTS
        };
        assert_eq!(ledger.view_stack(Cell::Craft(slots[0])).unwrap().count, 3);
        assert_eq!(ledger.view_stack(Cell::Craft(slots[1])).unwrap().count, 4);
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    request_id: request,
                    status: StackResponseStatus::Rejected,
                    containers: Arc::from([]),
                }]),
            },
        ));
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(ledger.view_stack(Cell::Craft(slots[0])).unwrap().count, 5);
    }
}

#[test]
fn repeated_trade_offers_preserve_pending_item_requests() {
    let mut ledger = ledger(true);
    let request = ledger.begin_trade().unwrap();
    let state = ledger.trading().unwrap();
    let event = TradeEvent {
        container: ContainerIdentity::window(7),
        actor_unique_id: 90,
        title: Arc::clone(&state.title),
        modern: true,
        tier: state.tier,
        offers: state
            .offers
            .iter()
            .map(|offer| offer.offer.clone())
            .collect(),
    };
    ledger.apply(&InventoryEvent::Trade(event));
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
}

#[test]
fn classic_trade_navigation_skips_locked_offers() {
    let mut ledger = ledger(false);
    let trade = ledger.storage.as_mut().unwrap().trade.as_mut().unwrap();
    let first = trade.offers[0].clone();
    trade.offers = [0, 1, 0]
        .into_iter()
        .enumerate()
        .map(|(index, tier)| {
            let mut offer = first.clone();
            offer.offer.network_id += index as u32;
            offer.offer.tier = tier;
            offer
        })
        .collect();

    assert!(ledger.cycle_trade(false).is_err());
    ledger.cycle_trade(true).unwrap();
    assert_eq!(ledger.trading().unwrap().selected, Some(2));
    assert!(ledger.cycle_trade(true).is_err());
    ledger.cycle_trade(false).unwrap();
    assert_eq!(ledger.trading().unwrap().selected, Some(0));
}

#[test]
fn exhausted_or_locked_trades_do_not_consume_items() {
    for locked in [false, true] {
        let mut ledger = ledger(true);
        let state = ledger.storage.as_mut().unwrap().trade.as_mut().unwrap();
        let offer = &mut Arc::make_mut(&mut state.offers)[0].offer;
        if locked {
            offer.tier = 1;
        } else {
            offer.uses = offer.max_uses;
        }
        assert!(ledger.begin_trade().is_err());
        assert!(ledger.pending_request_id().is_none());
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(
            ledger
                .view_stack(Cell::Craft(protocol::MODERN_TRADE_SLOTS[0]))
                .unwrap()
                .count,
            5
        );
    }
}

#[test]
fn trade_ingredients_accept_the_auxiliary_wildcard() {
    let mut ledger = ledger(true);
    let state = ledger.storage.as_mut().unwrap().trade.as_mut().unwrap();
    for input in Arc::make_mut(&mut state.offers)[0]
        .inputs
        .iter_mut()
        .flatten()
    {
        input.metadata = u32::from(protocol::RECIPE_ANY_AUX);
    }
    assert!(ledger.trade_output().is_some());
    assert!(ledger.begin_trade().is_ok());
}

#[test]
fn registry_replacement_rebinds_every_trade_preview() {
    let mut ledger = ledger(true);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        7,
        64,
        0,
    ));
    let trade = &ledger.trading().unwrap().offers[0];
    assert_eq!(trade.output.as_ref().unwrap().network_id, 7);
    assert!(
        trade
            .inputs
            .iter()
            .flatten()
            .all(|input| input.network_id == 7)
    );
}

#[test]
fn one_ingredient_can_be_paid_from_either_or_both_inputs() {
    for modern in [false, true] {
        for counts in [[0, 5], [1, 1], [5, 0], [5, 5]] {
            let mut ledger = ledger(modern);
            let trade = ledger.storage.as_mut().unwrap().trade.as_mut().unwrap();
            let offer = &mut Arc::make_mut(&mut trade.offers)[0];
            offer.offer.inputs[1] = None;
            offer.inputs[1] = None;
            let slots = if modern {
                protocol::MODERN_TRADE_SLOTS
            } else {
                protocol::LEGACY_TRADE_SLOTS
            };
            for (index, slot) in slots.iter().enumerate() {
                let stack = NetworkItemStack {
                    network_id: 6,
                    stack_network_id: index as i32 + 20,
                    count: counts[index],
                    ..NetworkItemStack::empty()
                };
                ledger
                    .confirmed
                    .set(Cell::Craft(*slot), super::super::cells::Held::new(&stack));
            }
            assert_eq!(
                [ledger.trade_input_valid(0), ledger.trade_input_valid(1)],
                counts.map(|count| count != 0)
            );
            assert!(!ledger.trade_input_valid(2));
            assert!(ledger.begin_trade().is_ok());
            assert_eq!(ledger.cursor_stack().unwrap().count, 1);
        }
    }
}

#[test]
fn block_item_purchases_preserve_the_authoritative_result_identity() {
    let mut ledger = ledger(true);
    let window = ledger.storage.as_mut().unwrap();
    let offer = &mut Arc::make_mut(&mut window.trade.as_mut().unwrap().offers)[0];
    offer.offer.output.block_descriptor = Some(Arc::from([0]));
    assert!(ledger.trade_output().is_none());
    let result = NetworkItemStack {
        network_id: 6,
        count: 1,
        block_runtime_id: 123,
        extra_data: Arc::from([0; 10]),
        ..NetworkItemStack::empty()
    };
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
            slot: u16::from(protocol::CREATED_OUTPUT_SLOT),
        },
        stack: result.clone(),
        storage_item: None,
    }));
    assert_eq!(ledger.trade_output().unwrap().block_runtime_id, 123);
    ledger.begin_trade().unwrap();
    assert!(PlayerInventoryLedger::same_item(
        ledger.cursor_stack().unwrap(),
        &result
    ));
}

#[test]
fn purchases_reserve_stock_until_rejected_and_commit_it_when_accepted() {
    let mut ledger = ledger(true);
    let state = ledger.storage.as_mut().unwrap().trade.as_mut().unwrap();
    Arc::make_mut(&mut state.offers)[0].offer.uses = 1;
    let rejected = ledger.begin_trade().unwrap();
    assert!(ledger.trade_output().is_none());
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                request_id: rejected,
                status: StackResponseStatus::Rejected,
                containers: Arc::from([]),
            }]),
        },
    ));
    assert!(ledger.trade_output().is_some());
    let accepted = ledger.begin_trade().unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    let response = InventoryEvent::Response(protocol::ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            request_id: accepted,
            status: StackResponseStatus::Accepted,
            containers: Arc::from([]),
        }]),
    });
    ledger.apply(&response);
    ledger.apply(&response);
    assert_eq!(ledger.trading().unwrap().offers[0].offer.uses, 2);
    assert!(ledger.trade_output().is_none());
}

#[test]
fn refreshed_stock_is_not_changed_again_by_an_older_purchase_response() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let mut ledger = ledger(true);
        let id = ledger.begin_trade().unwrap();
        assert!(ledger.mark_transport_enqueued(10));
        let state = ledger.trading().unwrap();
        let mut offers: Vec<_> = state
            .offers
            .iter()
            .map(|offer| offer.offer.clone())
            .collect();
        offers[0].uses = 1;
        ledger.apply(&InventoryEvent::Trade(TradeEvent {
            container: ContainerIdentity::window(7),
            actor_unique_id: 90,
            title: Arc::from("Trader"),
            modern: true,
            tier: 0,
            offers: offers.into(),
        }));
        ledger.apply(&InventoryEvent::Response(
            protocol::ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    request_id: id,
                    status,
                    containers: Arc::from([]),
                }]),
            },
        ));
        assert_eq!(ledger.trading().unwrap().offers[0].offer.uses, 1);
        assert!(ledger.trade_offer_available(0));
    }
}

#[test]
fn unsent_purchase_stock_rolls_back_after_a_same_window_refresh() {
    for refresh in [false, true] {
        let mut ledger = ledger(true);
        Arc::make_mut(
            &mut ledger
                .storage
                .as_mut()
                .unwrap()
                .trade
                .as_mut()
                .unwrap()
                .offers,
        )[0]
        .offer
        .max_uses = 1;
        let id = ledger.begin_trade().unwrap();
        if refresh {
            let state = ledger.trading().unwrap();
            let mut offers: Vec<_> = state
                .offers
                .iter()
                .map(|offer| offer.offer.clone())
                .collect();
            offers[0].uses = 0;
            ledger.apply(&InventoryEvent::Trade(TradeEvent {
                container: ContainerIdentity::window(7),
                actor_unique_id: 90,
                title: Arc::from("Trader"),
                modern: true,
                tier: 0,
                offers: offers.into(),
            }));
        }
        assert!(!ledger.trade_offer_available(0));
        ledger.abandon_requests(|request| request.request_id == id);
        assert!(ledger.trade_offer_available(0));
        assert_eq!(ledger.trading().unwrap().offers[0].offer.uses, 0);
    }
}

/// Refreshes the fixture's stock without replacing its window or offer identities.
fn refresh_stock(ledger: &mut PlayerInventoryLedger, uses: i32) {
    let window = ledger.storage.as_ref().unwrap();
    let state = window.trade.as_ref().unwrap();
    let mut offers: Vec<_> = state
        .offers
        .iter()
        .map(|offer| offer.offer.clone())
        .collect();
    offers[0].uses = uses;
    let event = TradeEvent {
        container: ContainerIdentity::window(window.window_id),
        actor_unique_id: window.actor_unique_id,
        title: Arc::clone(&state.title),
        modern: window.kind == WindowKind::Trade { modern: true },
        tier: state.tier,
        offers: offers.into(),
    };
    ledger.apply(&InventoryEvent::Trade(event));
}

/// Delivers one original response for the current fixture request.
fn answer(ledger: &mut PlayerInventoryLedger, id: i32, status: StackResponseStatus) {
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([StackResponse {
                request_id: id,
                status,
                containers: Arc::from([]),
            }]),
        },
    ));
}

#[test]
fn an_unchanged_refresh_keeps_an_admitted_purchase_reserved() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let mut ledger = ledger(true);
        Arc::make_mut(
            &mut ledger
                .storage
                .as_mut()
                .unwrap()
                .trade
                .as_mut()
                .unwrap()
                .offers,
        )[0]
        .offer
        .max_uses = 1;
        let id = ledger.begin_trade().unwrap();
        assert!(ledger.mark_transport_enqueued(10));
        refresh_stock(&mut ledger, 0);
        assert!(!ledger.trade_offer_available(0));
        answer(&mut ledger, id, status);
        assert_eq!(
            ledger.trade_offer_available(0),
            status == StackResponseStatus::Rejected
        );
    }
}

#[test]
fn refreshed_stock_counts_out_of_order_purchase_results_once() {
    for rejection_first in [false, true] {
        let mut ledger = ledger(true);
        let first = ledger.begin_trade().unwrap();
        assert!(ledger.mark_transport_enqueued(10));
        ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        let second = ledger.begin_trade().unwrap();
        assert!(ledger.mark_transport_enqueued(30));
        refresh_stock(&mut ledger, 1);
        if rejection_first {
            answer(&mut ledger, first, StackResponseStatus::Rejected);
        }
        answer(&mut ledger, second, StackResponseStatus::Accepted);
        if !rejection_first {
            answer(&mut ledger, first, StackResponseStatus::Rejected);
        }
        assert_eq!(ledger.trading().unwrap().offers[0].offer.uses, 1);
        assert!(ledger.trade_offer_available(0));
    }
}

#[test]
fn stock_observations_do_not_credit_later_purchases() {
    let mut ledger = ledger(true);
    let first = ledger.begin_trade().unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    refresh_stock(&mut ledger, 1);
    answer(&mut ledger, first, StackResponseStatus::Rejected);
    let second = ledger.begin_trade().unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    assert!(!ledger.trade_offer_available(0));
    answer(&mut ledger, second, StackResponseStatus::Accepted);
    assert_eq!(ledger.trading().unwrap().offers[0].offer.uses, 2);
    assert!(!ledger.trade_offer_available(0));
}

#[test]
fn closing_trade_refresh_does_not_reopen_or_retire_its_purchase() {
    let mut ledger = ledger(true);
    let request = ledger.begin_trade().unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.request_storage_close();
    let generation = ledger.storage.as_ref().unwrap().generation;
    refresh_stock(&mut ledger, 0);
    assert!(!ledger.storage_inventory_desired_open());
    assert_eq!(ledger.storage.as_ref().unwrap().generation, generation);
    assert_eq!(ledger.pending_request_id(), Some(request));
    answer(&mut ledger, request, StackResponseStatus::Accepted);
    assert_ne!(ledger.pending_request_id(), Some(request));
}

#[test]
fn refreshing_the_selected_recipe_clears_its_old_authoritative_preview() {
    for replace_identity in [false, true] {
        let mut ledger = ledger(true);
        ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: u16::from(protocol::CREATED_OUTPUT_SLOT),
            },
            stack: NetworkItemStack {
                network_id: 6,
                count: 1,
                ..NetworkItemStack::empty()
            },
            storage_item: None,
        }));
        assert!(ledger.created_output_stack().is_some());
        let trade = ledger.trading().unwrap();
        let mut offer = trade.offers[0].offer.clone();
        if replace_identity {
            offer.network_id += 1;
        } else {
            offer.output.count += 1;
        }
        ledger.apply(&InventoryEvent::Trade(TradeEvent {
            container: ContainerIdentity::window(7),
            actor_unique_id: 90,
            title: Arc::clone(&trade.title),
            modern: true,
            tier: 0,
            offers: Arc::from([offer]),
        }));
        assert!(ledger.created_output_stack().is_none());
    }
}
