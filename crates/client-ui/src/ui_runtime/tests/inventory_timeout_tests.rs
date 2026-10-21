use super::*;
use inventory::inventory_ledger::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, PERSONAL_INVENTORY_WINDOW_TYPE,
};
use protocol::{
    ContainerCloseEvent, ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryEvent,
};

#[test]
fn personal_control_timeouts_release_admission_without_guessing_a_window_id() {
    for closing in [false, true] {
        for desired_open in [false, true] {
            let mut player = player_state::PlayerState::new(1);
            let mut runtime = UiRuntime::new(1);
            player
                .inventory
                .ledger_mut()
                .apply(&InventoryEvent::Authority(InventoryAuthority::Server));
            runtime
                .publish_local_runtime_id(&mut player, 1, 42)
                .unwrap();
            runtime.toggle_inventory(&mut player, None);
            assert!(runtime.inventory_open());
            assert!(player.inventory.ledger_mut().mark_transport_enqueued(0));
            let open = InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            });
            if closing {
                runtime
                    .enqueue_inventory_event(&mut player, 1, 1, open.clone())
                    .unwrap();
                runtime.drain_pending_inventory(&mut player);
                runtime.toggle_inventory(&mut player, None);
                assert!(player.inventory.ledger_mut().mark_transport_enqueued(0));
            }
            if runtime.inventory_open() != desired_open {
                runtime.toggle_inventory(&mut player, None);
            }
            runtime.inventory_pointer_gui = desired_open.then_some([20.0, 30.0]);
            runtime.poll_inventory_timeout(&mut player, INVENTORY_REQUEST_TIMEOUT_MILLIS);
            let expected_open = desired_open;
            assert_eq!(runtime.inventory_open(), expected_open);
            assert_eq!(
                player.inventory.ledger().personal_inventory_desired_open(),
                expected_open
            );
            assert_eq!(
                runtime.inventory_pointer_gui(),
                expected_open.then_some([20.0, 30.0])
            );

            runtime
                .enqueue_inventory_event(
                    &mut player,
                    1,
                    2,
                    InventoryEvent::Close(ContainerCloseEvent {
                        container: ContainerIdentity::window(protocol::PLAYER_INVENTORY_WINDOW_ID),
                        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                        server_initiated: false,
                    }),
                )
                .unwrap();
            runtime.drain_pending_inventory(&mut player);
            assert_eq!(runtime.inventory_open(), expected_open);
            if !closing {
                assert!(player.inventory.ledger().pending_batch().unwrap().is_none());
                if runtime.inventory_open() {
                    runtime.toggle_inventory(&mut player, None);
                }
                runtime.toggle_inventory(&mut player, None);
            }
            if !closing || desired_open {
                assert!(
                    player
                        .inventory
                        .ledger_mut()
                        .mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1)
                );
                runtime
                    .enqueue_inventory_event(&mut player, 1, 3, open)
                    .unwrap();
                runtime.drain_pending_inventory(&mut player);
                assert!(runtime.inventory_open());
            } else {
                assert!(player.inventory.ledger().pending_batch().unwrap().is_none());
            }
        }
    }
}
