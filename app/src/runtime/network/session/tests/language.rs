use super::*;
use crate::runtime::network::{
    drain_network_controls, publish_bootstrap_inventory,
    resource_packs::{
        BootstrapGenerationDisposition, classify_bootstrap_generation, install_server_language,
    },
};
use crate::ui_runtime::UiRuntime;

fn overlay(value: &[u8]) -> Arc<assets::ServerLangOverlay> {
    assets::ServerLangOverlay::read(value.len(), |target| {
        target.copy_from_slice(value);
        true
    })
    .unwrap()
}

#[test]
fn only_current_successful_bootstrap_can_install_or_retire_language() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(2);

    let mut runtime = UiRuntime::new(2);
    let old = overlay(b"item.stone.name=Old\n");
    let old_weak = Arc::downgrade(&old);
    assert_eq!(
        classify_bootstrap_generation(2, 1, 2),
        BootstrapGenerationDisposition::Expected
    );
    let accepted = publish_bootstrap_inventory(
        &mut player_runtime,
        &mut runtime,
        None,
        InventoryEvent::Authority(InventoryAuthority::Server),
    );
    assert!(accepted);
    install_server_language(&mut runtime, 2, Some(old), accepted);
    assert_eq!(runtime.localized_item_name("minecraft:stone"), "Old");
    runtime.begin_session(&mut player_runtime, 3);
    assert!(old_weak.upgrade().is_none());
    let current = overlay(b"item.stone.name=Current\n");
    install_server_language(&mut runtime, 3, Some(current), true);
    let late = overlay(b"item.stone.name=Late\n");
    let late_weak = Arc::downgrade(&late);
    assert_eq!(
        classify_bootstrap_generation(3, 3, 2),
        BootstrapGenerationDisposition::Stale
    );
    install_server_language(&mut runtime, 2, Some(late), true);
    assert!(late_weak.upgrade().is_none());
    assert_eq!(runtime.localized_item_name("minecraft:stone"), "Current");
    let failed = overlay(b"item.stone.name=Failed\n");
    let failed_weak = Arc::downgrade(&failed);
    let accepted = publish_bootstrap_inventory(
        &mut player_runtime,
        &mut runtime,
        None,
        InventoryEvent::SelectedSlot(protocol::SelectedSlotEvent {
            container: protocol::ContainerIdentity::window(0),
            slot: 0,
            select_slot: true,
        }),
    );
    assert!(!accepted);
    install_server_language(&mut runtime, 3, Some(failed), accepted);
    assert!(failed_weak.upgrade().is_none());
    assert_eq!(runtime.localized_item_name("minecraft:stone"), "Stone");
}

#[test]
fn replacing_handle_drops_old_terminal_receiver_before_new_session_publication() {
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
        let old = std::mem::replace(&mut handle, NetworkHandle::disconnected());
        drop(old);
        assert!(sender.is_closed());
        assert!(drain_network_controls(handle.control_events_mut(), 64).is_empty());
        let mut runtime = UiRuntime::new(3);
        install_server_language(
            &mut runtime,
            3,
            Some(overlay(b"item.stone.name=Current\n")),
            true,
        );
        assert_eq!(runtime.localized_item_name("minecraft:stone"), "Current");
    }
}
