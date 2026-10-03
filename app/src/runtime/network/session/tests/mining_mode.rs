use super::*;
use crate::{
    runtime::network::{
        drain_network_controls, publish_bootstrap_inventory,
        resource_packs::{BootstrapGenerationDisposition, classify_bootstrap_generation},
    },
    ui_runtime::UiRuntime,
};

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
    runtime.clear_block_breaking_mode(&mut player_runtime);
    assert!(publish_bootstrap_inventory(
        &mut player_runtime,
        &mut runtime,
        None,
        InventoryEvent::Authority(InventoryAuthority::Server),
    ));
    runtime.install_block_breaking_mode(&mut player_runtime, 7, false, true);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(false)
    );
    assert_eq!(
        classify_bootstrap_generation(7, 7, 6),
        BootstrapGenerationDisposition::Stale
    );
    runtime.install_block_breaking_mode(&mut player_runtime, 6, true, true);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        Some(false)
    );
    runtime.clear_block_breaking_mode(&mut player_runtime);
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
    runtime.install_block_breaking_mode(&mut player_runtime, 7, true, false);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        None
    );
    // A later equipment/physics/FIFO fault uses the same final success gate.
    runtime.install_block_breaking_mode(&mut player_runtime, 7, true, true);
    runtime.clear_block_breaking_mode(&mut player_runtime);
    runtime.install_block_breaking_mode(&mut player_runtime, 7, false, false);
    assert_eq!(
        runtime.server_authoritative_block_breaking(&player_runtime),
        None
    );
    let source = include_str!("../../../network.rs").replace("\r\n", "\n");
    let clear = source
        .find("ui_runtime.clear_block_breaking_mode(&mut player_runtime);")
        .unwrap();
    let equipment = source
        .find("let routed = match publish_equipment_identity(")
        .unwrap();
    let physics = source.find("physics_authority.apply_start_game(").unwrap();
    let fifo = source
        .find("world FIFO rejected buffered equipment:")
        .unwrap();
    let install = source
        .find("ui_runtime.install_block_breaking_mode(")
        .unwrap();
    assert!(clear < physics && physics < equipment && equipment < fifo && fifo < install);
    assert!(source[install..].starts_with(
        "ui_runtime.install_block_breaking_mode(\n                    &mut player_runtime,\n                    session_generation,\n                    server_authoritative_block_breaking,\n                    client_world.fatal_error.is_none(),",
    ));
    for terminal in [
        "NetworkControlEvent::Failed {",
        "NetworkControlEvent::Transferred {",
        "NetworkControlEvent::Stopped {",
    ] {
        let branch = &source[source.find(terminal).unwrap()..];
        assert!(
            branch
                .find("ui_runtime.clear_block_breaking_mode(&mut player_runtime);")
                .unwrap()
                < branch.find("movement.deactivate();").unwrap()
        );
    }
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
        handle.control_events = receiver;
        sender.try_send(terminal).unwrap();
        drop(std::mem::replace(
            &mut handle,
            NetworkHandle::disconnected(),
        ));
        assert!(sender.is_closed());
        assert!(drain_network_controls(handle.control_events_mut(), 64).is_empty());
        let mut runtime = UiRuntime::new(8);
        runtime.install_block_breaking_mode(&mut player_runtime, 8, false, true);
        assert_eq!(
            runtime.server_authoritative_block_breaking(&player_runtime),
            Some(false)
        );
        runtime.clear_block_breaking_mode(&mut player_runtime);
        assert_eq!(
            runtime.server_authoritative_block_breaking(&player_runtime),
            None
        );
    }
}
