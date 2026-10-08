use super::super::{NetworkCommand, PacketSendError};
use super::NetworkHandle;

#[test]
fn production_form_enqueue_binds_generation_and_distinguishes_definite_backpressure() {
    let (mut network, _) = NetworkHandle::stub();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    network.commands = sender;
    network.session_generation = 42;
    let packet = || protocol::modal_form_cancel_response(7);
    assert!(matches!(
        network.send_form_packet(41, packet()),
        Err(PacketSendError::Closed(_))
    ));
    assert!(matches!(
        network.send_form_packet(0, packet()),
        Err(PacketSendError::Closed(_))
    ));
    assert!(receiver.try_recv().is_err());
    network.send_form_packet(42, packet()).unwrap();
    assert!(matches!(
        network.send_form_packet(42, packet()),
        Err(PacketSendError::Full(_))
    ));
    let NetworkCommand::Send {
        packet: accepted, ..
    } = receiver.try_recv().unwrap()
    else {
        panic!("form enqueue must submit a packet");
    };
    let session = protocol::BedrockSession { shield_item_id: 0 };
    assert_eq!(
        protocol::encode(&accepted, &session).unwrap(),
        protocol::encode(&packet(), &session).unwrap()
    );
    assert!(
        receiver.try_recv().is_err(),
        "Full enqueue was definitely not accepted"
    );
    drop(receiver);
    assert!(matches!(
        network.send_form_packet(42, packet()),
        Err(PacketSendError::Closed(_))
    ));
}

#[test]
fn render_distance_settings_send_is_session_fenced_and_retries_after_backpressure() {
    let (mut network, _) = NetworkHandle::stub();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    network.commands = sender;
    network.session_generation = 42;
    let packet = || protocol::request_chunk_radius_packet(8, u8::MAX);
    assert!(matches!(
        network.send_settings_packet(41, packet()),
        Err(PacketSendError::Closed(_))
    ));
    network.send_settings_packet(42, packet()).unwrap();
    assert!(matches!(
        network.send_settings_packet(42, packet()),
        Err(PacketSendError::Full(_))
    ));
    let NetworkCommand::Send {
        packet: accepted, ..
    } = receiver.try_recv().unwrap()
    else {
        panic!("settings must enqueue a packet send");
    };
    let session = protocol::BedrockSession { shield_item_id: 0 };
    assert_eq!(
        protocol::encode(&accepted, &session).unwrap(),
        protocol::encode(&packet(), &session).unwrap()
    );
    network.send_settings_packet(42, packet()).unwrap();
}

#[test]
fn boss_subscription_packets_use_the_session_fenced_fifo_and_retry_without_loss() {
    let (mut network, _) = NetworkHandle::stub();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    network.commands = sender;
    network.session_generation = 42;
    let event = protocol::BossEvent {
        target_entity_id: -17,
        action: protocol::BossAction::Show,
        title: "Dragon".into(),
        filtered_title: "Filtered dragon".into(),
        progress: 0.75,
        style: protocol::BossStyle {
            color: protocol::BossColor::Purple,
            overlay: protocol::BossOverlay::Progress,
            darken_sky: None,
            create_world_fog: None,
        },
    };
    let show = protocol::boss_registration_response(&event).unwrap();
    let hide = protocol::boss_registration_response(&protocol::BossEvent {
        action: protocol::BossAction::Hide,
        ..event
    })
    .unwrap();
    assert!(matches!(
        network.send_form_packet(41, show.clone()),
        Err(PacketSendError::Closed(_))
    ));
    assert!(receiver.try_recv().is_err());
    network.send_form_packet(42, show.clone()).unwrap();
    assert!(matches!(
        network.send_form_packet(42, hide.clone()),
        Err(PacketSendError::Full(_))
    ));
    let NetworkCommand::Send {
        packet: accepted, ..
    } = receiver.try_recv().unwrap()
    else {
        panic!("boss subscription must submit a packet");
    };
    assert_eq!(accepted, show);
    assert!(receiver.try_recv().is_err());
    network.send_form_packet(42, hide.clone()).unwrap();
    let NetworkCommand::Send {
        packet: accepted, ..
    } = receiver.try_recv().unwrap()
    else {
        panic!("boss removal must submit a packet");
    };
    assert_eq!(accepted, hide);
    drop(receiver);
    assert!(matches!(
        network.send_form_packet(42, hide),
        Err(PacketSendError::Closed(_))
    ));
}
