use std::sync::Arc;

use super::*;

fn event(session: u64, sequence: u64) -> SequencedAudioEvent {
    SequencedAudioEvent {
        origin_stream_session_id: session,
        dimension: 0,
        dimension_epoch: 0,
        actor_synchronization: None,
        sequence,
        event: protocol::AudioEvent::Play(protocol::PlayAudioEvent {
            name: Arc::from(TARGET),
            position: [9, -17, 25],
            volume: 1.0,
            pitch: 1.0,
            loop_count: -1,
            server_sound_handle: Some(123456789),
        }),
    }
}

#[test]
fn exact_selector_is_off_by_default_and_rejects_other_values() {
    for value in [None, Some(""), Some("true"), Some("other.sound")] {
        let mut evidence = WireEvidence::new(selected(value));
        evidence.bind(Some(1));
        assert!(evidence.observe(1, &event(1, 1)).is_none());
        assert!(evidence.rows.is_empty());
    }
    assert!(selected(Some(TARGET)));
}

#[test]
fn four_rows_are_bounded_and_session_disconnect_resets_the_budget() {
    let mut evidence = WireEvidence::new(true);
    evidence.bind(Some(1));
    for sequence in 1..=9 {
        assert_eq!(
            evidence.observe(1, &event(1, sequence)).is_some(),
            sequence <= 4
        );
    }
    assert_eq!(evidence.rows.len(), 4);
    evidence.bind(None);
    assert!(evidence.rows.is_empty());
    evidence.bind(Some(2));
    assert!(evidence.observe(2, &event(2, 1)).is_some());
}

#[test]
fn producer_session_and_strict_fifo_are_required_without_rebinding() {
    let mut evidence = WireEvidence::new(true);
    evidence.bind(Some(2));
    assert!(evidence.observe(2, &event(1, 500)).is_none());
    assert!(evidence.observe(1, &event(1, 500)).is_none());
    assert!(evidence.observe(2, &event(2, 3)).is_some());
    for sequence in [3, 2, 1] {
        assert!(evidence.observe(2, &event(2, sequence)).is_none());
    }
    let mut unrelated = event(2, 4);
    let protocol::AudioEvent::Play(play) = &mut unrelated.event else {
        unreachable!()
    };
    play.name = Arc::from("unrelated.test.sound");
    assert!(evidence.observe(2, &unrelated).is_none());
    assert!(evidence.observe(2, &event(2, 4)).is_none());
    assert!(evidence.observe(2, &event(2, 5)).is_some());
    assert_eq!(evidence.rows.len(), 2);
}

#[test]
fn serialization_has_only_fixed_decoded_fields_and_handle_presence() {
    let mut evidence = WireEvidence::new(true);
    evidence.bind(Some(1));
    let row = evidence.observe(1, &event(1, 1)).unwrap();
    let value = serde_json::to_value(row).unwrap();
    let keys: std::collections::BTreeSet<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        std::collections::BTreeSet::from([
            "schema",
            "authority",
            "origin_stream_session_id",
            "observed_fifo_sequence",
            "position_eighth_blocks",
            "position_blocks",
            "loop_count",
            "gain_bits",
            "pitch_bits",
            "server_sound_handle_present",
        ])
    );
    assert_eq!(
        value["position_eighth_blocks"],
        serde_json::json!([9, -17, 25])
    );
    assert_eq!(
        value["position_blocks"],
        serde_json::json!([1.125, -2.125, 3.125])
    );
    assert_eq!(value["loop_count"], -1);
    assert_eq!(value["gain_bits"], 1.0_f32.to_bits());
    assert_eq!(value["pitch_bits"], 1.0_f32.to_bits());
    assert_eq!(value["server_sound_handle_present"], true);
    let text = serde_json::to_string(row).unwrap();
    for forbidden in [
        "123456789",
        TARGET,
        "account",
        "address",
        "payload",
        "packet_bytes",
    ] {
        assert!(!text.contains(forbidden));
    }
    let mut absent = event(1, 2);
    let protocol::AudioEvent::Play(play) = &mut absent.event else {
        unreachable!()
    };
    play.server_sound_handle = None;
    let row = evidence.observe(1, &absent).unwrap();
    assert!(!row.server_sound_handle_present);
}

#[test]
fn review_broken_stdout_cannot_panic_the_audio_session() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(std::panic::catch_unwind(|| write_marker(&mut Broken, "{}")).is_ok());
}
