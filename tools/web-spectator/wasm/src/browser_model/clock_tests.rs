use std::time::Duration;

use serde_json::json;

use super::{Frame, event_age_millis};

fn frame(playback: serde_json::Value) -> Frame {
    let mut value = json!({
        "id": "scene", "arenaId": "arena", "updatedAt": "2026-10-02T00:00:00Z",
        "players": []
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(playback.as_object().unwrap().clone());
    Frame::parse(&value.to_string()).unwrap()
}

#[test]
fn paused_recording_keeps_its_pose_and_presentation_time_without_heartbeats() {
    let frame = frame(json!({"replayPlaying": false}));
    let elapsed = Duration::from_secs(600);
    assert!(!frame.is_stale(elapsed));
    assert_eq!(frame.presentation_millis(1_000, elapsed), 1_000);
    assert_eq!(
        frame.interpolation_fraction(Duration::ZERO, Duration::from_millis(100)),
        1.0
    );
}

#[test]
fn playback_speed_scales_the_shared_presentation_clock() {
    for (speed, expected) in [(0.25, 1_250), (1.0, 2_000), (2.0, 3_000)] {
        let frame = frame(json!({"replayPlaying": true, "replaySpeed": speed}));
        assert_eq!(
            frame.presentation_millis(1_000, Duration::from_secs(1)),
            expected
        );
        assert!(!frame.is_stale(Duration::from_secs(600)));
    }
}

#[test]
fn live_observations_keep_the_existing_clock_and_timeout() {
    let frame = frame(json!({}));
    assert_eq!(
        frame.presentation_millis(1_000, Duration::from_millis(250)),
        1_250
    );
    assert!(!frame.is_stale(Duration::from_secs(5)));
    assert!(frame.is_stale(Duration::from_secs(6)));
}

#[test]
fn historical_events_age_against_recorded_time_and_freeze_on_pause() {
    // This epoch belongs to the recording and does not depend on the viewer's date.
    let recorded_event_millis = 1_000_000_u64;
    let frame = frame(json!({"replayPlaying": false}));
    let timeline = frame.presentation_millis(recorded_event_millis + 100, Duration::from_secs(600));
    assert_eq!(
        event_age_millis(recorded_event_millis as f64, timeline),
        Some(100.0)
    );
    assert_eq!(event_age_millis(f64::NAN, timeline), None);
}

#[test]
fn playback_cadence_is_applied_once_to_position_interpolation() {
    let frame = frame(json!({"replayPlaying": true, "replaySpeed": 2.0}));
    assert_eq!(
        frame.interpolation_fraction(Duration::from_millis(25), Duration::from_millis(50)),
        0.5
    );
}
