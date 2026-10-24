use super::*;

/// A rocket aimed into air swings only after its click-air batch is admitted.
#[test]
fn a_gliding_rocket_air_use_admits_hand_feedback_atomically() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let rocket = UseFrame {
        ready: true,
        ..item_frame(100, true, stack(2, 401, 3), "minecraft:firework_rocket")
    };
    runtime.observe_press(true);
    step_and_send(&mut runtime, &mut swings, &rocket, 7, 6, |_| {
        Err(BatchSendError::Full)
    });
    assert_eq!(swings.take_started(), None);
    assert!(runtime.take_inventory_change().is_none());
    assert!(!runtime.take_rocket_boost());
    let mut packets = Vec::new();
    step_and_send(&mut runtime, &mut swings, &rocket, 7, 6, |sent| {
        packets = sent;
        Ok(())
    });
    assert_eq!(swings.take_started(), Some(6));
    assert_eq!(packets.len(), 2);
    // A rocket boost swings as an item use, not a throw.
    assert_eq!(
        protocol::test_support::packets::swing(&wire(&packets[0])),
        Some((7, Some("useitem".to_owned())))
    );
    assert_eq!(use_kind(&packets[1]), "use");
    assert!(!runtime.is_using());
    assert!(runtime.take_inventory_change().is_none());
    // The boost is predicted locally, not left to a server movement effect.
    assert!(runtime.take_rocket_boost());
    assert!(!runtime.take_rocket_boost());
}

/// Air use outside a glide still reaches the server without predicting a launch.
#[test]
fn a_grounded_rocket_air_use_does_not_predict_hand_feedback() {
    let mut runtime = ItemUseRuntime::default();
    let mut swings = SwingTracker::default();
    let rocket = UseFrame {
        ready: false,
        ..item_frame(100, true, stack(2, 401, 3), "minecraft:firework_rocket")
    };
    runtime.observe_press(true);
    let mut packets = Vec::new();
    step_and_send(&mut runtime, &mut swings, &rocket, 7, 6, |sent| {
        packets = sent;
        Ok(())
    });
    assert_eq!(swings.take_started(), None);
    assert_eq!(packets.len(), 1);
    assert_eq!(use_kind(&packets[0]), "use");
    assert!(runtime.take_inventory_change().is_none());
    assert!(!runtime.take_rocket_boost());
}
