use super::*;
use crate::runtime::network::{
    drain_network_controls, publish_bootstrap_inventory,
    resource_packs::{BootstrapGenerationDisposition, classify_bootstrap_generation},
};
use client_ui::ui_runtime::UiRuntime;

#[test]
fn mining_mode_requires_current_complete_bootstrap_and_failed_repeat_stays_unknown() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(7);

    let mut runtime = UiRuntime::new(7);
    assert_eq!(
        classify_bootstrap_generation(7, 6, 7),
        BootstrapGenerationDisposition::Expected
    );
    assert_eq!(
        classify_bootstrap_generation(7, 7, 7),
        BootstrapGenerationDisposition::Stale
    );
    player_runtime.facts.clear_block_breaking_mode();
    assert!(publish_bootstrap_inventory(
        &mut player_runtime,
        &mut runtime,
        None,
        InventoryEvent::Authority(InventoryAuthority::Server),
    ));
    player_runtime
        .facts
        .install_block_breaking_mode(7, false, true);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(false)
    );
    assert_eq!(
        classify_bootstrap_generation(7, 7, 6),
        BootstrapGenerationDisposition::Stale
    );
    player_runtime
        .facts
        .install_block_breaking_mode(6, true, true);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        Some(false)
    );
    player_runtime.facts.clear_block_breaking_mode();
    assert!(!publish_bootstrap_inventory(
        &mut player_runtime,
        &mut runtime,
        None,
        InventoryEvent::SelectedSlot(protocol::SelectedSlotEvent {
            container: protocol::ContainerIdentity::window(0),
            slot: 0,
            select_slot: true,
        }),
    ));
    player_runtime
        .facts
        .install_block_breaking_mode(7, true, false);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        None
    );
    // A later equipment/physics/FIFO fault uses the same final success gate.
    player_runtime
        .facts
        .install_block_breaking_mode(7, true, true);
    player_runtime.facts.clear_block_breaking_mode();
    player_runtime
        .facts
        .install_block_breaking_mode(7, false, false);
    assert_eq!(
        player_runtime.facts.server_authoritative_block_breaking(),
        None
    );
}

#[test]
fn old_terminal_receiver_cannot_clear_new_mining_mode() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(8);

    for terminal in [
        NetworkControlEvent::Stopped {
            decode_error_count: 0,
        },
        NetworkControlEvent::Transferred {
            target: SessionTransferTarget {
                host: "localhost".into(),
                port: 19132,
            },
            decode_error_count: 0,
        },
        NetworkControlEvent::Failed {
            message: "fixture failure".into(),
            decode_error_count: 0,
            server_disconnect: None,
            origin: NetworkFailureOrigin::Receive,
        },
    ] {
        let (mut handle, _) = NetworkHandle::stub();
        let (sender, receiver) = mpsc::channel(1);
        *handle.control_events_mut() = receiver;
        sender.try_send(terminal).unwrap();
        drop(std::mem::replace(
            &mut handle,
            NetworkHandle::disconnected(),
        ));
        assert!(sender.is_closed());
        assert!(drain_network_controls(handle.control_events_mut(), 64).is_empty());
        player_runtime
            .facts
            .install_block_breaking_mode(8, false, true);
        assert_eq!(
            player_runtime.facts.server_authoritative_block_breaking(),
            Some(false)
        );
        player_runtime.facts.clear_block_breaking_mode();
        assert_eq!(
            player_runtime.facts.server_authoritative_block_breaking(),
            None
        );
    }
}
