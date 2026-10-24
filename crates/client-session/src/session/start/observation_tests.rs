use super::*;

/// Exercises the actual handle's actor/generation/terminal fences and full-RTT projection.
#[test]
fn native_rtt_is_unknown_or_scoped_to_original_session_and_actor() {
    let (mut handle, channels) = network_channels::<()>(SessionGeneration::new(7));
    let (zero_publisher, zero_receiver) = protocol::test_support::rtt_observation();
    zero_publisher.publish(Some(Duration::from_millis(30)));
    let zero_actor = channels
        .connection_observation
        .install(0, Some(zero_receiver));
    assert_eq!(
        handle.round_trip_ms(7, 0),
        None,
        "an absent local actor is never measured"
    );
    drop(zero_actor);
    let (publisher, receiver) = protocol::test_support::rtt_observation();
    let observation = channels.connection_observation.install(42, Some(receiver));
    assert_eq!(handle.round_trip_ms(7, 42), None);
    publisher.publish(Some(Duration::from_micros(24_001)));
    assert_eq!(
        handle.round_trip_ms(7, 42),
        Some(25),
        "full RTT is rounded, never doubled"
    );
    assert_eq!(handle.round_trip_ms(6, 42), None);
    assert_eq!(handle.round_trip_ms(7, 43), None);
    publisher.publish(Some(Duration::MAX));
    assert_eq!(handle.round_trip_ms(7, 42), Some(u32::MAX));
    publisher.publish(Some(Duration::ZERO));
    assert_eq!(handle.round_trip_ms(7, 42), None);
    publisher.publish(Some(Duration::from_millis(30)));
    let (replacement, _) = network_channels::<()>(SessionGeneration::new(8));
    assert_eq!(
        replacement.round_trip_ms(8, 42),
        None,
        "replacement has no original connection authority"
    );
    handle.shutdown();
    publisher.publish(Some(Duration::from_millis(99)));
    assert_eq!(handle.round_trip_ms(7, 42), None);
    drop(observation);
}

/// A canceled owner or dropped adapter cannot leave an apparently live last measurement.
#[test]
fn native_rtt_withdraws_on_owner_completion_and_abrupt_publisher_drop() {
    let (handle, channels) = network_channels::<()>(SessionGeneration::new(7));
    let (publisher, receiver) = protocol::test_support::rtt_observation();
    publisher.publish(Some(Duration::from_millis(30)));
    let observation = channels.connection_observation.install(42, Some(receiver));
    assert_eq!(handle.round_trip_ms(7, 42), Some(30));
    drop(publisher);
    assert_eq!(
        handle.round_trip_ms(7, 42),
        None,
        "dropped publisher retained its last Some sample"
    );
    drop(observation);
    let (publisher, receiver) = protocol::test_support::rtt_observation();
    publisher.publish(Some(Duration::from_millis(40)));
    let observation = channels.connection_observation.install(42, Some(receiver));
    assert_eq!(handle.round_trip_ms(7, 42), Some(40));
    drop(observation);
    assert_eq!(
        handle.round_trip_ms(7, 42),
        None,
        "completed or canceled pump retained its observation"
    );
}
